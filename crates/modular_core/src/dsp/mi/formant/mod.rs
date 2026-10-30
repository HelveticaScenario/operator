mod grainlet;

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
        utils::voct_to_hz,
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::{GlideMark, SwapGlide},
};

use grainlet::{GrainletControls, GrainletPair};

/// Plaits' output gain for this engine, at ±5 V full scale.
const OUTPUT_GAIN: f32 = -0.7 * 5.0;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct FormantParams {
    /// pitch in V/Oct (0V = C4): how often the grains repeat
    #[signal(type = pitch)]
    freq: PolySignal,
    /// formant frequency in V/Oct, independent of `freq` (default 0.5, ≈370 Hz)
    #[signal(type = frequency, default = 0.5, range = (-3.0, 4.0))]
    #[deserr(default)]
    formant: Option<PolySignal>,
    /// second formant, 0–5: two octaves below the first (0) to two octaves
    /// above (5); below 2.5 the bare window bleeds in (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    ratio: Option<PolySignal>,
    /// window shape, 0–5: narrow pulse (0) to broad and flat-topped (5);
    /// narrows on its own at high pitches (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    width: Option<PolySignal>,
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
struct FormantOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Granular formant oscillator, from Mutable Instruments Plaits.
///
/// Every cycle of **freq** starts a short grain: a sine at the **formant**
/// frequency inside a window. Two of these grains sound together, the second
/// at a **ratio** of the first's formant, giving vowel-like and resonant
/// timbres whose formants stay put as the pitch moves.
///
/// - **formant** — V/Oct, the first formant's frequency.
/// - **ratio** — 0–5, the second formant from two octaves below (0) to two
///   above (5). Below 2.5 the window itself bleeds in, adding the fundamental.
/// - **width** — 0–5, the window's shape, from a narrow pulse to a broad,
///   flat-topped one. It narrows by itself as the pitch rises past ≈2 kHz.
///
/// Through-zero FM plays each grain's window backward.
///
/// ```js
/// $mi.formant('c2', { formant: $sine('0.1hz').range(0, 2) }).out()
/// ```
#[module(name = "$mi.formant", args(freq), has_init, block)]
pub struct Formant {
    outputs: FormantOutputs,
    state: FormantState,
    channel_state: Box<[Voice]>,
    params: FormantParams,
}

#[derive(Default)]
struct FormantState {
    clock: ControlClock,
    inv_sample_rate: f32,
    /// Reciprocal of the pitch, in cycles per sample, at which `width` has
    /// fully narrowed.
    narrowing: f32,
    max_carrier: f32,
    max_formant: f32,
}

impl FormantState {
    fn new(sample_rate: f32) -> Self {
        Self {
            clock: ControlClock::default(),
            inv_sample_rate: 1.0 / sample_rate,
            narrowing: sample_rate / 2000.0,
            max_carrier: 0.5 * max_frequency(sample_rate),
            max_formant: max_frequency(sample_rate),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Voice {
    osc: GrainletPair,
    dc_blocker: OnePole,
    /// Set by the first tick, which jumps the controls to their targets.
    started: bool,
    formant: SwapGlide,
    ratio: SwapGlide,
    width: SwapGlide,
}

/// The control inputs at a tick: the raw signals and their glide marks.
struct Knobs {
    formant: (f32, GlideMark),
    ratio: (f32, GlideMark),
    width: (f32, GlideMark),
}

impl Voice {
    fn tick(&mut self, f0: f32, knobs: Knobs, length: f32, state: &FormantState) {
        // Bounded at Nyquist, so an extreme pitch cannot destabilise the DC
        // blocker.
        let f0 = limit_frequency(f0, 0.5);
        let formant = self
            .formant
            .follow(knobs.formant.0, knobs.formant.1, CONTROL_RATE);
        let ratio = self
            .ratio
            .follow(knobs.ratio.0, knobs.ratio.1, CONTROL_RATE);
        let width = self
            .width
            .follow(knobs.width.0, knobs.width.1, CONTROL_RATE);

        let f1 = voct_to_hz(formant) * state.inv_sample_rate;
        let harmonics = (ratio * 0.2).clamp(0.0, 1.0);
        let morph = (width * 0.2).clamp(0.0, 1.0);
        let ratio = (4.0 * harmonics - 2.0).exp2();
        let bleed = if harmonics < 0.5 {
            1.0 - 2.0 * harmonics
        } else {
            0.0
        };
        let controls = GrainletControls {
            formant: [
                f1.min(state.max_formant),
                (f1 * ratio).min(state.max_formant),
            ],
            shape: 0.33 + (morph - 0.33) * (1.0 - f0.abs() * state.narrowing).max(0.0),
            bleed: bleed * (2.0 - bleed),
        };
        self.osc.set(&controls, self.started.then_some(length));
        self.started = true;
        self.dc_blocker.set_g(tan_dirty(0.3 * f0.abs()));
    }

    #[inline(always)]
    fn step(&mut self, f0: f32, state: &FormantState) -> f32 {
        let f0 = limit_frequency(f0, state.max_carrier);
        OUTPUT_GAIN * self.dc_blocker.high_pass(self.osc.step(f0))
    }
}

impl Formant {
    fn init(&mut self, sample_rate: f32) {
        self.state = FormantState::new(sample_rate);
    }

    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let (tick, _) = self.state.clock.advance(1, sample_rate);
        let p = &self.params;
        for (ch, voice) in self.channel_state[..channels].iter_mut().enumerate() {
            let f0 = apply_fm(
                p.freq.get_value(ch),
                p.fm.value_or(ch, FormantParams::FM_DEFAULT),
                p.fm_mode,
            ) * self.state.inv_sample_rate;
            if let Some(length) = tick {
                let knobs = Knobs {
                    formant: (
                        p.formant.value_or(ch, FormantParams::FORMANT_DEFAULT),
                        p.formant.glide_mark(ch),
                    ),
                    ratio: (
                        p.ratio.value_or(ch, FormantParams::RATIO_DEFAULT),
                        p.ratio.glide_mark(ch),
                    ),
                    width: (
                        p.width.value_or(ch, FormantParams::WIDTH_DEFAULT),
                        p.width.glide_mark(ch),
                    ),
                };
                voice.tick(f0, knobs, length, &self.state);
            }
            self.outputs.sample.set(ch, voice.step(f0, &self.state));
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut FormantBlockOutputs,
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
                        formant: (inputs.formant.row(i)[ch], p.formant.glide_mark(ch)),
                        ratio: (inputs.ratio.row(i)[ch], p.ratio.glide_mark(ch)),
                        width: (inputs.width.row(i)[ch], p.width.glide_mark(ch)),
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

message_handlers!(impl Formant {});

#[cfg(test)]
mod tests;
