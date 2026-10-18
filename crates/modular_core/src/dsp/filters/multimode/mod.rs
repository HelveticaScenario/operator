//! `$multimode` — one channel of Mutable Instruments Blades, modelled from its
//! published schematic (blades_v40).
//!
//! Signal path: drive VCA → zener-clamped transimpedance amp → clean/wavefolded
//! crossfade → 2164-based state-variable filter → LP/BP/HP morphing VCAs.
//! Every V2164 control port here is driven through a linearising servo or a
//! precision rectifier whose diode keeps the control voltage ≥ 0 V, so each
//! VCA's gain is clamped to at most unity except the drive VCA, which has no
//! such diode.

mod components;
mod curve;
mod drive;
mod output;
mod svf;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::{
        cached::Cached,
        halfband::{Halfband2xDown, Halfband2xUp},
        one_pole::OnePole,
        rng::{LcgRng, seed_base},
        sanitize, voct_to_hz,
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
};

use components::{ZENER_PAIR_CAPACITANCE_0V, vca_hd2};
use drive::{DriveState, PreEmphasis};
use output::{OUTPUT_POLE_HZ, output_amp, rails};
use svf::{CoreTuning, NEGATIVE_DAMPING, SvfCore};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct MultimodeParams {
    /// signal input (DC-coupled)
    input: PolySignal,
    /// cutoff frequency in V/Oct (0V = C4), capped at ~21 kHz
    #[signal(type = frequency, default = 0.0, range = (-5.0, 5.0))]
    cutoff: PolySignal,
    /// resonance (0-5). Self-oscillates from ~4.3.
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    resonance: Option<PolySignal>,
    /// response: 0 = lowpass, 2.5 = bandpass, 5 = highpass, crossfading between
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    mode: Option<PolySignal>,
    /// input drive (0 = unity, 5 = +18 dB), cubic response
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    drive: Option<PolySignal>,
    /// drive response (0 = soft clipping, 5 = two-stage wavefolder)
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    fold: Option<PolySignal>,
    /// when true (default), the circuit runs at twice the engine rate, which
    /// reduces aliasing from the clipper, wavefolder and self-oscillation and
    /// keeps the highpass accurate at high cutoffs; false costs about 40% less
    /// CPU
    #[serde(default = "default_true")]
    #[deserr(default = default_true())]
    high_res: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MultimodeOutputs {
    #[output("output", "response selected by mode", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
    #[output("lp", "lowpass (the output at mode 0)", range = (-10.0, 10.0))]
    lp: PolyOutput,
    #[output("bp", "bandpass (the output at mode 2.5)", range = (-10.0, 10.0))]
    bp: PolyOutput,
    #[output("hp", "highpass (the output at mode 5)", range = (-10.0, 10.0))]
    hp: PolyOutput,
}

/// Nominal cutoff at unity VCA gain: 1 / (2π · 30 kΩ · 220 pF). The zener
/// capacitance across the first integrator lowers the actual resonance below it.
const FC_MAX_HZ: f32 = 24_114.0;
/// Lowest cutoff the VCAs reach at their −100 dB floor.
const FC_MIN_HZ: f32 = FC_MAX_HZ * 1.0e-5;

/// V2164 control law: gain = 10^(−Vc / (20 · 33 mV)).
const VCA_VOLTS_PER_DECADE: f32 = 20.0 * 0.033;

/// Resonance control (IC19D): R148 / R155 = 20k / 100k, so a 5 V knob gives 1 V.
const RES_CONTROL_SCALE: f32 = 0.2;

/// Drive servo (IC2B/IC4A): the reference VCA's gain is linear in the drive
/// voltage, 1 at unity and rising 0.2 per volt (R32 / R22 against the 5 V
/// reference).
const DRIVE_SERVO_SCALE: f32 = 1.0 / 5.0;
/// The reference VCA sees the servo output through R27/R41 loaded by its 5k
/// control port, a 0.331 tap, so the signal VCA's gain is the servo gain cubed.
const DRIVE_EXPONENT: f32 = 3.02;
/// V2164 maximum gain, +20 dB.
const DRIVE_MAX_GAIN: f32 = 10.0;

/// R80 is a 10 kΩ log pot whose wiper grounds the clean path (R89 → R10) on one
/// side and the folded path (R88 → R3) on the other. Taper: 10 % at mid-travel.
const FOLD_POT_OHMS: f32 = 10.0e3;
const FOLD_TAPER_BASE: f32 = 81.0;

/// Mode servos: gain = mode / 2.5 V less a reference offset (R146 for the
/// BP/HP VCA; R170's 10 MΩ trickle for the LP-to-BP VCA).
const MODE_SCALE: f32 = 1.0 / 2.5;
const MODE_OFFSET_A: f32 = 1.0;
const MODE_OFFSET_B: f32 = 0.02;
/// Mode VCA input resistors (R96, R98, and the R111/R118 summer's equivalent).
const R_MODE_IN: f32 = 62.0e3;
/// Drive VCA input resistor (R33).
const R_DRIVE_IN: f32 = 30.0e3;
/// Drive transimpedance feedback: R24 (30k) across C12 (18 pF) and the D4/D5
/// zener pair at its 0 V capacitance.
const DRIVE_POLE_HZ: f32 =
    1.0 / (std::f32::consts::TAU * 30.0e3 * (18.0e-12 + ZENER_PAIR_CAPACITANCE_0V));
/// Circuit noise at the filter's summing node (~10 µV). Self-oscillation grows
/// from it, so the filter starts ringing without any input, as the hardware does.
const NOISE_FLOOR_V: f32 = 1.0e-5;

/// Per-channel caches of the control laws. The integrator gains also depend on
/// the core rate; [`ChannelState::configure`] clears them when it changes.
#[derive(Default, Clone, Copy)]
struct Controls {
    drive_gain: Cached<f32>,
    fold_gains: Cached<(f32, f32)>,
    tuning: Cached<CoreTuning>,
    res_gain: Cached<f32>,
}

/// With `highRes` the whole circuit runs at twice the engine rate, and the
/// four outputs are decimated separately.
#[derive(Default, Clone, Copy)]
struct ChannelState {
    /// Rate the core and its caches are configured for; 0 until configured.
    core_rate: f32,
    controls: Controls,
    up: Halfband2xUp,
    down: [Halfband2xDown; 4],
    drive_pole: OnePole,
    output_poles: [OnePole; 3],
    drive: DriveState,
    core: SvfCore,
    noise: LcgRng,
    pre_emphasis: PreEmphasis,
}

impl ChannelState {
    /// Sets the core's rate-dependent coefficients, clearing the oversampling
    /// filters and integrator cache when the rate changes.
    fn configure(&mut self, rate: f32) {
        if self.core_rate == rate {
            return;
        }
        self.core_rate = rate;
        self.controls.tuning = Cached::default();
        self.up = Halfband2xUp::default();
        self.down = Default::default();
        self.pre_emphasis = PreEmphasis::default();
        let coeff = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / rate).exp();
        self.drive_pole.set_coeff(coeff(DRIVE_POLE_HZ));
        for pole in &mut self.output_poles {
            pole.set_coeff(coeff(OUTPUT_POLE_HZ));
        }
    }

    /// One core-rate sample from drive VCA output `x`, returning the outputs
    /// `[output, lp, bp, hp]`. The output amp's feedback pole filters the LP,
    /// BP and HP nodes before the mode VCAs, which only rescale them, so
    /// filtering each one equals filtering their mix.
    #[inline]
    fn tick(
        &mut self,
        x: f32,
        fold_gains: (f32, f32),
        tuning: CoreTuning,
        res_gain: f32,
        mode_gains: (f32, f32),
    ) -> [f32; 4] {
        let x = self.drive_pole.process(x);
        let filter_in = self.drive.process(x, fold_gains.0, fold_gains.1);
        let u = NOISE_FLOOR_V * self.noise.next_bipolar() - filter_in;
        let (lp, bp) = self.core.step(u, tuning, res_gain);
        let hp = -filter_in - lp - NEGATIVE_DAMPING * bp;
        let [p_lp, p_bp, p_hp] = &mut self.output_poles;
        let (lp, bp, hp) = (p_lp.process(lp), p_bp.process(bp), p_hp.process(hp));
        [
            output_amp(morph(lp, bp, hp, mode_gains)),
            output_amp(lp),
            output_amp(-bp),
            output_amp(hp),
        ]
    }
}

/// Mutable Instruments Blades–style multimode filter with drive and wavefolder.
///
/// A circuit model of one channel of Blades: a 2164 VCA state-variable filter
/// that morphs continuously from lowpass through bandpass to highpass, preceded
/// by a drive VCA into a zener soft clipper and a two-stage diode wavefolder.
/// The signal path is DC-coupled, so it can process CVs too.
///
/// - **cutoff** — V/Oct. Tracks exactly at low cutoffs and runs progressively
///   flat in the top octaves (−15 cents at 1 kHz, about half a semitone at
///   5 kHz), topping out near 21 kHz, as the circuit does. Responds at audio
///   rate, so an oscillator patched in gives exponential FM.
/// - **resonance** — 0–5. Self-oscillates above ~4.3; the bandpass is then
///   held near ±4 V by the soft knee of the integrator's zeners. With no input,
///   `mode` shifts the phase of the sine.
/// - **mode** — 0 lowpass, 2.5 bandpass, 5 highpass; values between crossfade.
/// - **drive** — input gain ahead of everything else: 0 is unity, 5 is about
///   +18 dB. The zeners' soft knee already rounds a ±5 V input at 0.
/// - **fold** — 0 soft clipping only, 5 the two-stage wavefolder.
/// - **highRes** — on by default. Off runs the circuit at the engine rate for
///   about 40% less CPU, with more aliasing from hot drive, folding and
///   self-oscillation, and a highpass that attenuates more below the cutoff
///   once the cutoff is above ~8 kHz (about 2 dB more at 15 kHz).
///
/// Passband gain in lowpass mode is about +2.8 dB at unity drive, rolling off
/// above ~20 kHz at the output amp, whose zeners compress the output softly
/// above about 6 V.
///
/// ```js
/// // resonant sweep that morphs from lowpass to highpass
/// $multimode($saw('c2'), $sine('0.1hz').range(-1, 2), 3.8, $sine('0.07hz').range(0, 5)).out()
/// // wavefolded sine through a gentle lowpass
/// $multimode($sine('c3'), 'c6', 1, 0, { drive: 3, fold: 3 }).out()
/// // no input: a self-oscillating sine at A3
/// $multimode(0, 'a3', 5).out()
/// // without oversampling, for less CPU
/// $multimode($saw('c2'), 'c5', 2, 0, { highRes: false }).out()
/// ```
#[module(
    name = "$multimode",
    args(input, cutoff, resonance, mode),
    has_init,
    patch_update
)]
pub struct Multimode {
    outputs: MultimodeOutputs,
    state: MultimodeState,
    channel_state: Box<[ChannelState]>,
    params: MultimodeParams,
}

#[derive(Default)]
struct MultimodeState {
    sample_rate: f32,
}

/// Gains of the two R80 crossfade paths into the filter, as `(clean, folded)`.
#[inline]
fn fold_gains(fold: f32) -> (f32, f32) {
    let t = (fold * 0.2).clamp(0.0, 1.0);
    let r_fold = FOLD_POT_OHMS * (FOLD_TAPER_BASE.powf(t) - 1.0) / (FOLD_TAPER_BASE - 1.0);
    let r_clean = FOLD_POT_OHMS - r_fold;
    let par = |a: f32, b: f32| a * b / (a + b).max(f32::MIN_POSITIVE);
    // Each path is a T network into the IC2A virtual ground (R5 = 200k).
    let clean = {
        let shunt = par(r_clean, 62.0e3);
        200.0e3 * shunt / ((30.0e3 + shunt) * 62.0e3)
    };
    let folded = {
        let shunt = par(r_fold, 5.1e3);
        200.0e3 * shunt / ((5.1e3 + shunt) * 5.1e3)
    };
    (clean, folded)
}

#[inline]
fn vca_gain(control_v: f32) -> f32 {
    10.0_f32.powf(-control_v / VCA_VOLTS_PER_DECADE)
}

/// Gains `(a, b)` of the two mode VCAs: `a` blends HP into the BP path, `b`
/// crossfades from LP to it.
#[inline]
fn mode_gains(mode: f32) -> (f32, f32) {
    (
        (mode * MODE_SCALE - MODE_OFFSET_A).clamp(0.0, 1.0),
        (mode * MODE_SCALE - MODE_OFFSET_B).clamp(0.0, 1.0),
    )
}

/// The mode VCAs' mix into the output amp, in volts across R173. VCA A takes
/// BP and HP through 62k each; VCA B takes its output less BP and LP (through
/// the IC14C summer) and adds to the LP that reaches the output directly.
#[inline]
fn morph(lp: f32, bp: f32, hp: f32, (gain_a, gain_b): (f32, f32)) -> f32 {
    let a = gain_a * (bp + hp) * vca_hd2(gain_a, (bp + hp) / R_MODE_IN);
    let w = a - bp - lp;
    lp + gain_b * w * vca_hd2(gain_b, w / R_MODE_IN)
}

impl Multimode {
    fn init(&mut self, sample_rate: f32) {
        components::prime();
        drive::prime();
        output::prime();
        self.state.sample_rate = sample_rate;
        let base = seed_base(self);
        for (i, channel) in self.channel_state.iter_mut().enumerate() {
            channel.noise.seed(base, i);
        }
    }

    fn core_rate(&self) -> f32 {
        if self.params.high_res {
            2.0 * self.state.sample_rate
        } else {
            self.state.sample_rate
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();
        let rate = self.core_rate();
        let p = &self.params;

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            let controls = &mut state.controls;
            // Each control law sanitizes its input, so a non-finite control acts
            // as 0 V rather than reaching the filter state.
            let drive_gain = controls.drive_gain.get(p.drive.value_or(ch, 0.0), |drive| {
                let servo = (1.0 + sanitize(drive) * DRIVE_SERVO_SCALE).max(0.0);
                servo.powf(DRIVE_EXPONENT).min(DRIVE_MAX_GAIN)
            });
            let fold = controls
                .fold_gains
                .get(p.fold.value_or(ch, 0.0), |fold| fold_gains(sanitize(fold)));
            let tuning = controls.tuning.get(p.cutoff.get_value(ch), |voct| {
                let fc = voct_to_hz(sanitize(voct)).clamp(FC_MIN_HZ, FC_MAX_HZ);
                CoreTuning::new(fc, fc / FC_MAX_HZ, rate)
            });
            let res_gain = controls
                .res_gain
                .get(p.resonance.value_or(ch, 0.0), |resonance| {
                    vca_gain((sanitize(resonance) * RES_CONTROL_SCALE).max(0.0))
                });
            let modes = mode_gains(sanitize(p.mode.value_or(ch, 0.0)));

            let input = p.input.get_value(ch);
            let x = sanitize(drive_gain * input * vca_hd2(drive_gain, input / R_DRIVE_IN));
            let out = if p.high_res {
                let (even, odd) = state.up.process(x);
                let even = state.tick(even, fold, tuning, res_gain, modes);
                let odd = state.tick(odd, fold, tuning, res_gain, modes);
                std::array::from_fn(|k| state.down[k].process(even[k], odd[k]))
            } else {
                let x = state.pre_emphasis.process(x);
                state.tick(x, fold, tuning, res_gain, modes)
            };

            self.outputs.sample.set(ch, rails(out[0]));
            self.outputs.lp.set(ch, rails(out[1]));
            self.outputs.bp.set(ch, rails(out[2]));
            self.outputs.hp.set(ch, rails(out[3]));
        }
    }
}

impl crate::types::PatchUpdateHandler for Multimode {
    fn on_patch_update(&mut self) {
        let rate = self.core_rate();
        for channel in self.channel_state.iter_mut() {
            channel.configure(rate);
        }
    }
}

message_handlers!(impl Multimode {});

#[cfg(test)]
mod tests;
