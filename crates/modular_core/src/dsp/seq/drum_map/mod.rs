//! Port of the drum-map mode of Mutable Instruments Grids
//! (grids/pattern_generator.cc, copyright 2011 Emilie Gillet, GPL-3.0-or-later).

mod nodes;

use deserr::{DeserializeError, Deserr, ErrorKind, IntoValue, ValuePointerRef};
use napi::Result;
use schemars::JsonSchema;

use crate::MonoSignal;
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{TempGate, TempGateState, min_gate_samples};
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
            deserr::Value::Float(f) if f == 16.0 => Ok(Self::Sixteenth),
            deserr::Value::Float(f) if f == 32.0 => Ok(Self::ThirtySecond),
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
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct DrumMapParams {
    /// playhead position; one cycle plays the 32-step pattern once
    #[default_connection(module = RootClock, port = "playhead", channels = [0, 1])]
    #[signal(range = (0.0, 1.0))]
    #[deserr(default)]
    playhead: Option<MonoSignal>,
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

#[derive(Default)]
struct DrumMapState {
    /// Step fired most recently; `None` until the first update or after a
    /// transport start, when only a step landed on exactly fires.
    last_step: Option<usize>,
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
        if let ClockMessages::Start = m {
            self.state.last_step = None;
        }
        Ok(())
    }

    fn update(&mut self, sample_rate: f32) {
        let playhead = self.params.playhead.value_or_zero();
        let position = (playhead - playhead.floor()) * STEPS_PER_PATTERN as f32;
        let mut step = (position as usize).min(STEPS_PER_PATTERN - 1);
        if self.params.resolution == DrumMapResolution::Sixteenth {
            step &= !1;
        }
        let fire = match self.state.last_step {
            Some(last) => last != step,
            None => position == step as f32,
        };
        self.state.last_step = Some(step);

        if fire {
            self.evaluate(step, min_gate_samples(sample_rate));
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

    fn evaluate(&mut self, step: usize, hold: u32) {
        let p = &self.params;
        for ch in 0..self.channel_count() {
            let state = &mut self.channel_state[ch];
            if step == 0 {
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
                    if level > ACCENT_LEVEL {
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
