mod z;

use std::ops::Range;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::{
        mi::core::{
            control::{CONTROL_RATE, ControlClock, for_each_stretch},
            filter::{OnePole, tan_dirty},
            phase::{limit_frequency, max_frequency},
        },
        oscillators::{FmMode, apply_fm},
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::{GlideMark, SwapGlide},
};

use z::{ZControls, ZOscillator};

/// Plaits' output gain for this engine, at ±5 V full scale.
const OUTPUT_GAIN: f32 = -0.6 * 5.0;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ZOscParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: PolySignal,
    /// resonance frequency, 0–5: at the pitch (0) up to eight octaves above
    /// (5) (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    cutoff: Option<PolySignal>,
    /// carrier contour, 0–5: from an unshaped pulse train (0) through a
    /// smooth window (2.5) to phase-shifted sines (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    shape: Option<PolySignal>,
    /// filter response, 0–5: low-pass, peaking, band-pass and high-pass
    /// characters in turn (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    mode: Option<PolySignal>,
    /// FM input signal (pre-scaled by user)
    #[signal(default = 0.0)]
    #[deserr(default)]
    fm: Option<PolySignal>,
    /// FM mode: throughZero (default), lin, or exp
    #[serde(default)]
    #[deserr(default)]
    fm_mode: FmMode,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ZOscOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Z oscillator, the filtered-waveform half of Mutable Instruments Plaits'
/// granular formant engine.
///
/// A sine at the **cutoff** frequency restarts every half cycle of **freq** and
/// is shaped by a decaying ramp, so it sounds like a waveform through a
/// resonant filter without any filter, and the resonance follows the pitch.
///
/// - **cutoff** — 0–5, the resonance from the pitch itself up to eight octaves
///   above it.
/// - **shape** — 0–5, the carrier's contour.
/// - **mode** — 0–5, the response, through low-pass, peaking, band-pass and
///   high-pass characters.
///
/// Through-zero FM runs the waveform backward.
///
/// ```js
/// $mi.zOsc('c2', { cutoff: $sine('0.2hz').range(1, 4), mode: 1 }).out()
/// ```
#[module(name = "$mi.zOsc", args(freq), has_init, block)]
pub struct ZOsc {
    outputs: ZOscOutputs,
    state: ZOscState,
    channel_state: Box<[Voice]>,
    params: ZOscParams,
}

#[derive(Default)]
struct ZOscState {
    clock: ControlClock,
    inv_sample_rate: f32,
    max_carrier: f32,
    max_formant: f32,
}

impl ZOscState {
    fn new(sample_rate: f32) -> Self {
        Self {
            clock: ControlClock::default(),
            inv_sample_rate: 1.0 / sample_rate,
            max_carrier: 0.5 * max_frequency(sample_rate),
            max_formant: max_frequency(sample_rate),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Voice {
    osc: ZOscillator,
    dc_blocker: OnePole,
    /// Set by the first tick, which jumps the controls to their targets.
    started: bool,
    cutoff: SwapGlide,
    shape: SwapGlide,
    mode: SwapGlide,
}

/// The control inputs at a tick: the raw signals and their glide marks.
struct Knobs {
    cutoff: (f32, GlideMark),
    shape: (f32, GlideMark),
    mode: (f32, GlideMark),
}

impl Voice {
    fn tick(&mut self, f0: f32, knobs: Knobs, length: f32, state: &ZOscState) {
        // Bounded at Nyquist, so an extreme pitch cannot destabilise the DC
        // blocker.
        let f0 = limit_frequency(f0, 0.5);
        let cutoff = self
            .cutoff
            .follow(knobs.cutoff.0, knobs.cutoff.1, CONTROL_RATE);
        let shape = self
            .shape
            .follow(knobs.shape.0, knobs.shape.1, CONTROL_RATE);
        let mode = self.mode.follow(knobs.mode.0, knobs.mode.1, CONTROL_RATE);
        let controls = ZControls {
            formant: (f0.abs() * (8.0 * (cutoff * 0.2).clamp(0.0, 1.0)).exp2())
                .min(state.max_formant),
            shape: (shape * 0.2).clamp(0.0, 1.0),
            mode: (mode * 0.2).clamp(0.0, 1.0),
        };
        self.osc.set(&controls, self.started.then_some(length));
        self.started = true;
        self.dc_blocker.set_g(tan_dirty(0.3 * f0.abs()));
    }

    #[inline(always)]
    fn step(&mut self, f0: f32, state: &ZOscState) -> f32 {
        let f0 = limit_frequency(f0, state.max_carrier);
        OUTPUT_GAIN * self.dc_blocker.high_pass(self.osc.step(f0))
    }
}

impl ZOsc {
    fn init(&mut self, sample_rate: f32) {
        self.state = ZOscState::new(sample_rate);
    }

    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let (tick, _) = self.state.clock.advance(1, sample_rate);
        let p = &self.params;
        for (ch, voice) in self.channel_state[..channels].iter_mut().enumerate() {
            let f0 = apply_fm(
                p.freq.get_value(ch),
                p.fm.value_or(ch, ZOscParams::FM_DEFAULT),
                p.fm_mode,
            ) * self.state.inv_sample_rate;
            if let Some(length) = tick {
                let knobs = Knobs {
                    cutoff: (
                        p.cutoff.value_or(ch, ZOscParams::CUTOFF_DEFAULT),
                        p.cutoff.glide_mark(ch),
                    ),
                    shape: (
                        p.shape.value_or(ch, ZOscParams::SHAPE_DEFAULT),
                        p.shape.glide_mark(ch),
                    ),
                    mode: (
                        p.mode.value_or(ch, ZOscParams::MODE_DEFAULT),
                        p.mode.glide_mark(ch),
                    ),
                };
                voice.tick(f0, knobs, length, &self.state);
            }
            self.outputs.sample.set(ch, voice.step(f0, &self.state));
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut ZOscBlockOutputs,
        range: Range<usize>,
        sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let inputs = p.block_inputs(range.clone(), channels, &mut scratch)?;
        let mut clock = self.state.clock;
        let state = &self.state;
        let voices = &mut self.channel_state[..channels];
        let inv_sample_rate = state.inv_sample_rate;
        let f0 = |i: usize, ch: usize| {
            apply_fm(inputs.freq.row(i)[ch], inputs.fm.row(i)[ch], p.fm_mode) * inv_sample_rate
        };
        for_each_stretch(&mut clock, range, sample_rate, |tick, stretch| {
            for (ch, voice) in voices.iter_mut().enumerate() {
                if let Some(length) = tick {
                    let i = stretch.start;
                    let knobs = Knobs {
                        cutoff: (inputs.cutoff.row(i)[ch], p.cutoff.glide_mark(ch)),
                        shape: (inputs.shape.row(i)[ch], p.shape.glide_mark(ch)),
                        mode: (inputs.mode.row(i)[ch], p.mode.glide_mark(ch)),
                    };
                    voice.tick(f0(i, ch), knobs, length, state);
                }
                for i in stretch.clone() {
                    outputs.sample.slot_mut(i)[ch] = voice.step(f0(i, ch), state);
                }
            }
        });
        self.state.clock = clock;
        Some(())
    }
}

message_handlers!(impl ZOsc {});

#[cfg(test)]
mod tests;
