//! Port of the drum-map mode of Mutable Instruments Grids
//! (grids/pattern_generator.cc, copyright 2011 Emilie Gillet, GPL-3.0-or-later).

mod nodes;

use deserr::{DeserializeError, Deserr, ErrorKind, IntoValue, ValuePointerRef};
use napi::Result;
use schemars::JsonSchema;

use crate::MonoSignal;
use crate::dsp::seq::FRESH_WINDOW_SECS;
use crate::dsp::seq::groove::{GrooveHold, before_first_step, fill_groove_inputs};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{GATE_HIGH_VOLTAGE, TempGate, TempGateState, min_gate_samples};
use crate::param_errors::ModuleParamErrors;
use crate::poly::{MonoSignalExt, PolyOutput, PolySignal, PolySignalExt};
use crate::types::ClockMessages;
use crate::types::PatchUpdateHandler;

use nodes::NODES;

const NUM_PARTS: usize = 3;
const STEPS_PER_PATTERN: usize = 32;
/// Map-level threshold above which a hit also fires its accent output.
const ACCENT_LEVEL: u8 = 192;

/// Node index at each corner of the 5×5 map, indexed `[x][y]`.
const DRUM_MAP: [[usize; 5]; 5] = [
    [10, 8, 0, 9, 11],
    [15, 7, 13, 12, 6],
    [18, 14, 4, 5, 3],
    [23, 16, 21, 1, 2],
    [24, 19, 17, 20, 22],
];

/// Steps per cycle, written in the DSL as the number `16` or `32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Connect, Default)]
enum DrumMapResolution {
    #[default]
    Sixteenth,
    ThirtySecond,
}

impl<E: DeserializeError> Deserr<E> for DrumMapResolution {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> std::result::Result<Self, E> {
        match value {
            deserr::Value::Integer(16) => Ok(Self::Sixteenth),
            deserr::Value::Integer(32) => Ok(Self::ThirtySecond),
            deserr::Value::Float(16.0) => Ok(Self::Sixteenth),
            deserr::Value::Float(32.0) => Ok(Self::ThirtySecond),
            _ => Err(deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected {
                    msg: "resolution must be 16 or 32".to_string(),
                },
                location,
            ))),
        }
    }
}

impl JsonSchema for DrumMapResolution {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("DrumMapResolution")
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "oneOf": [
                {
                    "const": 16,
                    "description": "16 steps per cycle, playing the map's even steps (Grids at 4 ppqn)"
                },
                {
                    "const": 32,
                    "description": "all 32 steps per cycle, including the map's ghost notes (Grids at 8 or 24 ppqn)"
                }
            ]
        }))
        .expect("DrumMapResolution schema is valid JSON Schema")
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = drum_map_fill_groove -> ModuleParamErrors)]
struct DrumMapParams {
    /// playhead position; one cycle plays the 32-step pattern once
    #[default_connection(module = RootClock, port = "playhead", channels = [0, 1])]
    #[signal(range = (0.0, 1.0))]
    #[deserr(default)]
    playhead: Option<MonoSignal>,
    /// groove offset added to the playhead to time each step; follows the
    /// global groove by default, and is 0 when `playhead` is set without it
    #[default_connection(module = RootClock, port = "grooveOffset", channel = 0)]
    #[deserr(default)]
    groove: Option<MonoSignal>,
    /// groove velocity (0-5V), sampled at each step: scales each hit's level
    /// for the accent threshold only, so lower velocities accent less; follows
    /// the global groove by default, and is full (5V) when `playhead` or
    /// `groove` is set without it
    #[default_connection(module = RootClock, port = "grooveVelocity", channel = 0)]
    #[signal(range = (0.0, 5.0))]
    #[deserr(default)]
    velocity: Option<MonoSignal>,
    /// horizontal position in the pattern map
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    x: Option<PolySignal>,
    /// vertical position in the pattern map
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    y: Option<PolySignal>,
    /// random perturbation of hit levels, re-rolled at the start of each cycle
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    chaos: Option<PolySignal>,
    /// bass drum fill density
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    bd_density: Option<PolySignal>,
    /// snare drum fill density
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    sd_density: Option<PolySignal>,
    /// hi-hat fill density
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    hh_density: Option<PolySignal>,
    /// step resolution
    #[serde(default)]
    #[deserr(default)]
    resolution: DrumMapResolution,
}

fn drum_map_fill_groove(
    mut params: DrumMapParams,
    _location: ValuePointerRef,
) -> std::result::Result<DrumMapParams, ModuleParamErrors> {
    fill_groove_inputs(&params.playhead, &mut params.groove, &mut params.velocity);
    Ok(params)
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DrumMapOutputs {
    #[output("bd", "bass drum trigger", default, range = (0.0, 5.0))]
    bd: PolyOutput,
    #[output("sd", "snare drum trigger", range = (0.0, 5.0))]
    sd: PolyOutput,
    #[output("hh", "hi-hat trigger", range = (0.0, 5.0))]
    hh: PolyOutput,
    #[output("bdAccent", "bass drum accent trigger", range = (0.0, 5.0))]
    bd_accent: PolyOutput,
    #[output("sdAccent", "snare drum accent trigger", range = (0.0, 5.0))]
    sd_accent: PolyOutput,
    #[output("hhAccent", "hi-hat accent trigger", range = (0.0, 5.0))]
    hh_accent: PolyOutput,
}

/// What the next update does with the step it lands on.
#[derive(Default, Clone, Copy, PartialEq)]
enum Sync {
    /// A fresh module's first sample: record the step and its position.
    #[default]
    Fresh,
    /// A fresh module's second sample: the playhead's speed reveals how long
    /// before creation the recorded step began, and the step plays one sample
    /// late if that is within [`FRESH_WINDOW_SECS`].
    Probing {
        /// Position within the cycle, in steps, on the first sample.
        position: f64,
    },
    /// Record the step without firing: a stopped transport stays silent until
    /// the next step boundary.
    Silent,
    /// Fire the step and start a new cycle: the transport just started, so the
    /// first sample lands in the downbeat.
    Restart,
    /// Fire on forward step changes, measured from `step`.
    Tracking,
}

#[derive(Default)]
struct DrumMapState {
    sync: Sync,
    /// Most recent step passed within the cycle, 0–31.
    step: usize,
    /// Keeps the grooved position from running backwards when the groove
    /// changes mid-play.
    groove_hold: GrooveHold,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    rng: LcgRng,
    seeded: bool,
    perturbation: [u8; NUM_PARTS],
    triggers: [TempGate; NUM_PARTS],
    accents: [TempGate; NUM_PARTS],
}

/// Topographic drum sequencer: a port of Mutable Instruments Grids.
///
/// Three trigger streams — bass drum, snare and hi-hat — are read from a
/// 5×5 map of drum patterns. **x** and **y** (0–5 V) move smoothly through
/// the map, interpolating between neighbouring patterns. Each part's
/// **density** sets how many of its possible hits play, from silence at 0 V
/// to every hit at 5 V. **chaos** adds random variation, re-rolled each
/// cycle. Strong hits also fire the matching accent output.
///
/// One cycle of the **playhead** plays the 32-step pattern once.
///
/// Steps follow the global groove set with `$setGroove`, and the groove's
/// step **velocity** decides how readily each hit accents: at full velocity
/// accents are as in Grids, and quieter steps accent only their strongest
/// hits. Pass a `$groove` as `groove` (with its `velocity`) to use a
/// different groove, or `groove: 0` to play straight.
///
/// ```js
/// const drums = $drumMap({ x: 1, y: 3, chaos: 0.5 })
/// // the bass drum is the default output
/// $sine('c1').amplitude($perc(drums, { decay: 0.3 })).out()
/// $noise('white').amplitude($perc(drums.hh, { decay: 0.05 })).out()
/// ```
#[module(name = "$drumMap", args(), patch_update)]
pub struct DrumMap {
    outputs: DrumMapOutputs,
    params: DrumMapParams,
    state: DrumMapState,
    channel_state: Box<[ChannelState]>,
}

message_handlers!(impl DrumMap {
    Clock(m) => DrumMap::on_clock_message,
});

/// Maps a 0–5 V knob voltage onto Grids' 8-bit parameter range.
fn knob(volts: f32) -> u8 {
    (volts * (255.0 / 5.0)).round().clamp(0.0, 255.0) as u8
}

fn u8_mix(a: u8, b: u8, balance: u8) -> u8 {
    ((a as u16 * (255 - balance) as u16 + b as u16 * balance as u16) >> 8) as u8
}

fn u8_mul_shift8(a: u8, b: u8) -> u8 {
    ((a as u16 * b as u16) >> 8) as u8
}

/// Hit level of `part` at `step`, bilinearly interpolated between the four
/// map nodes surrounding `(x, y)`.
fn read_drum_map(step: usize, part: usize, x: u8, y: u8) -> u8 {
    let i = (x >> 6) as usize;
    let j = (y >> 6) as usize;
    let offset = part * STEPS_PER_PATTERN + step;
    let a = NODES[DRUM_MAP[i][j]][offset];
    let b = NODES[DRUM_MAP[i + 1][j]][offset];
    let c = NODES[DRUM_MAP[i][j + 1]][offset];
    let d = NODES[DRUM_MAP[i + 1][j + 1]][offset];
    u8_mix(u8_mix(a, b, x << 2), u8_mix(c, d, x << 2), y << 2)
}

impl DrumMap {
    fn on_clock_message(&mut self, m: &ClockMessages) -> Result<()> {
        self.state.sync = match m {
            ClockMessages::Start => Sync::Restart,
            ClockMessages::Stop => Sync::Silent,
        };
        Ok(())
    }

    fn update(&mut self, sample_rate: f32) {
        // Summed in f64: the clock's bar count grows without bound, and an f32
        // sum would erode the fractional bar phase that selects the step.
        let straight = self
            .params
            .playhead
            .as_ref()
            .map_or(0.0, MonoSignal::get_value_f64);
        let playhead = self.state.groove_hold.apply(
            straight,
            straight + self.params.groove.value_or_zero() as f64,
        );
        // Before a late first step nothing plays, and a pending transport
        // start fires once that step arrives.
        if !before_first_step(straight, playhead) {
            self.advance_to(playhead, sample_rate);
        }

        for ch in 0..self.channel_count() {
            let state = &mut self.channel_state[ch];
            self.outputs.bd.set(ch, state.triggers[0].process());
            self.outputs.sd.set(ch, state.triggers[1].process());
            self.outputs.hh.set(ch, state.triggers[2].process());
            self.outputs.bd_accent.set(ch, state.accents[0].process());
            self.outputs.sd_accent.set(ch, state.accents[1].process());
            self.outputs.hh_accent.set(ch, state.accents[2].process());
        }
    }

    /// Move to grooved position `playhead` (cycles), playing the step it
    /// reaches when that step fires.
    fn advance_to(&mut self, playhead: f64, sample_rate: f32) {
        let position = (playhead - playhead.floor()) * STEPS_PER_PATTERN as f64;
        let stride = match self.params.resolution {
            DrumMapResolution::Sixteenth => 2,
            DrumMapResolution::ThirtySecond => 1,
        };
        let step = (position as usize).min(STEPS_PER_PATTERN - 1) / stride * stride;

        let last = self.state.step;
        let tracking = matches!(self.state.sync, Sync::Tracking | Sync::Probing { .. });
        // A backward jump over more than half the cycle is a wrap into the next
        // cycle (the clock's own bar-boundary rule). A rewind of at most one
        // step is jitter, such as a Link resync or a resolution change, and
        // keeps the previous step so it is not replayed; a longer rewind is a
        // seek and re-syncs silently.
        let (fire, new_cycle) = match self.state.sync {
            Sync::Fresh | Sync::Silent => (false, false),
            Sync::Restart => (true, true),
            Sync::Probing { position: first } if step == last => {
                let steps_per_sample = (position - first).rem_euclid(STEPS_PER_PATTERN as f64);
                let window = steps_per_sample * FRESH_WINDOW_SECS * sample_rate as f64;
                let on_boundary = steps_per_sample > 0.0 && first - last as f64 <= window;
                (on_boundary, on_boundary && step == 0)
            }
            _ if tracking && step > last => (true, false),
            _ if tracking && last - step > STEPS_PER_PATTERN / 2 => (true, true),
            _ => (false, false),
        };
        let jitter = tracking && step < last && last - step <= stride;
        if !jitter {
            self.state.step = step;
        }
        self.state.sync = match self.state.sync {
            Sync::Fresh => Sync::Probing { position },
            _ => Sync::Tracking,
        };

        if fire {
            let velocity = self.params.velocity.value_or(GATE_HIGH_VOLTAGE) / GATE_HIGH_VOLTAGE;
            self.evaluate(step, new_cycle, velocity, min_gate_samples(sample_rate));
        }
    }

    /// Play `step`. `velocity` (0–1) scales each hit's level against the
    /// accent threshold.
    fn evaluate(&mut self, step: usize, new_cycle: bool, velocity: f32, hold: u32) {
        let p = &self.params;
        for ch in 0..self.channel_count() {
            let state = &mut self.channel_state[ch];
            if new_cycle {
                let randomness = knob(p.chaos.value_or(ch, 0.0)) >> 2;
                for perturbation in state.perturbation.iter_mut() {
                    let byte = (state.rng.next_unit() * 256.0) as u8;
                    *perturbation = u8_mul_shift8(byte, randomness);
                }
            }

            let x = knob(p.x.value_or(ch, 2.5));
            let y = knob(p.y.value_or(ch, 2.5));
            let densities = [
                knob(p.bd_density.value_or(ch, 2.5)),
                knob(p.sd_density.value_or(ch, 2.5)),
                knob(p.hh_density.value_or(ch, 2.5)),
            ];
            for part in 0..NUM_PARTS {
                let level =
                    read_drum_map(step, part, x, y).saturating_add(state.perturbation[part]);
                if level > !densities[part] {
                    state.triggers[part].set_state(TempGateState::High, TempGateState::Low, hold);
                    if level as f32 * velocity > ACCENT_LEVEL as f32 {
                        state.accents[part].set_state(
                            TempGateState::High,
                            TempGateState::Low,
                            hold,
                        );
                    }
                }
            }
        }
    }
}

impl PatchUpdateHandler for DrumMap {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            if !state.seeded {
                state.rng.seed(base, ch);
                state.seeded = true;
            }
        }
    }
}

#[cfg(test)]
mod tests;
