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
mod lanes;
mod output;
mod svf;
mod vca;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::{
        rng::{LcgRng, seed_base},
        sanitize, voct_to_hz,
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
};

use components::{ZENER_PAIR_CAPACITANCE_0V, ZenerTables};
use curve::Curve;
use drive::{DriveCurves, DriveState, PreEmphasis};
use lanes::{HalfbandDown, HalfbandUp, LANES, LaneCache, Lanes, OnePole, f32x4};
use output::{OUTPUT_POLE_HZ, output_amp, rails};
use svf::{CoreTuning, NEGATIVE_DAMPING, SvfCore, TuningX4};
use vca::GainX4;

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

/// Control laws per lane, each stored in the form its stage reads and
/// recomputed only for lanes whose control changed. The integrator gains also
/// depend on the core rate; [`GroupState::configure`] clears them when it
/// changes.
#[derive(Default, Clone, Copy)]
struct Controls {
    drive: LaneCache,
    drive_x4: GainX4,
    fold: LaneCache,
    fold_gains: [(f32, f32); LANES],
    tuning: LaneCache,
    tuning_x4: TuningX4,
    res: LaneCache,
    res_x4: GainX4,
    mode: LaneCache,
    mode_x4: (GainX4, GainX4),
}

impl Controls {
    fn clear(&mut self) {
        self.drive.clear();
        self.fold.clear();
        self.tuning.clear();
        self.res.clear();
        self.mode.clear();
    }
}

/// The shared tables, fetched once per update so evaluations skip their
/// lazy-initialisation checks.
#[derive(Clone, Copy)]
struct Tables {
    zener: &'static ZenerTables,
    drive: &'static DriveCurves,
    chain: &'static Curve,
}

impl Tables {
    fn get() -> Self {
        Self {
            zener: components::zener_tables(),
            drive: drive::curves(),
            chain: output::chain(),
        }
    }
}

/// Four channels' state, one lane each; group `g` holds channels `4g..4g + 4`
/// and lives in `channel_state[g]`. With `highRes` the whole circuit runs at
/// twice the engine rate, and the four outputs are decimated separately.
#[derive(Default, Clone, Copy)]
struct GroupState {
    /// Rate the core and its caches are configured for; 0 until configured.
    core_rate: f32,
    controls: Controls,
    up: HalfbandUp,
    down: [HalfbandDown; 4],
    drive_pole: OnePole,
    output_poles: [OnePole; 3],
    drive: DriveState,
    core: SvfCore,
    noise: [LcgRng; LANES],
    pre_emphasis: PreEmphasis,
}

impl GroupState {
    /// Sets the core's rate-dependent coefficients, clearing the oversampling
    /// filters and integrator caches when the rate changes.
    fn configure(&mut self, rate: f32) {
        if self.core_rate == rate {
            return;
        }
        self.core_rate = rate;
        self.controls.tuning.clear();
        self.up = HalfbandUp::default();
        self.down = Default::default();
        self.pre_emphasis = PreEmphasis::default();
        let coeff = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / rate).exp();
        self.drive_pole.set_coeff(coeff(DRIVE_POLE_HZ));
        for pole in &mut self.output_poles {
            pole.set_coeff(coeff(OUTPUT_POLE_HZ));
        }
    }

    /// Returns lane `lane` to rest, as a fresh channel starts. Its noise
    /// generator keeps its seed.
    fn clear_lane(&mut self, lane: usize) {
        let c = &mut self.controls;
        c.drive.clear_lane(lane);
        c.fold.clear_lane(lane);
        c.tuning.clear_lane(lane);
        c.res.clear_lane(lane);
        c.mode.clear_lane(lane);
        self.up.clear_lane(lane);
        self.down.iter_mut().for_each(|d| d.clear_lane(lane));
        self.drive_pole.clear_lane(lane);
        self.output_poles
            .iter_mut()
            .for_each(|p| p.clear_lane(lane));
        self.drive.clear_lane(lane);
        self.core.clear_lane(lane);
        self.pre_emphasis.clear_lane(lane);
    }

    /// One core-rate sample from drive VCA outputs `x`, returning the outputs
    /// `[output, lp, bp, hp]`. The output amp's feedback pole filters the LP,
    /// BP and HP nodes before the mode VCAs, which only rescale them, so
    /// filtering each one equals filtering their mix. The drive stage runs on
    /// the first `active` lanes.
    #[inline]
    fn tick<L: Lanes>(
        &mut self,
        x: L,
        fold_gains: &[(f32, f32); LANES],
        tables: Tables,
        active: usize,
    ) -> [L; 4] {
        let x = self.drive_pole.process(x);
        let filter_in = self.drive.process(tables.drive, x, fold_gains, active);
        let mut noise = [0.0; LANES];
        for (noise, rng) in noise.iter_mut().zip(&mut self.noise).take(active) {
            *noise = rng.next_bipolar();
        }
        let u = L::splat(NOISE_FLOOR_V) * L::from_lanes(noise) - filter_in;
        let c = &self.controls;
        let (lp, bp) = self
            .core
            .step(u, &c.tuning_x4, &c.res_x4, tables.zener, active);
        let hp = -filter_in - lp - L::splat(NEGATIVE_DAMPING) * bp;
        let [p_lp, p_bp, p_hp] = &mut self.output_poles;
        let (lp, bp, hp) = (p_lp.process(lp), p_bp.process(bp), p_hp.process(hp));
        let amp = |v| output_amp(tables.chain, v, active);
        [
            amp(morph(lp, bp, hp, &c.mode_x4)),
            amp(lp),
            amp(-bp),
            amp(hp),
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
    channel_state: Box<[GroupState]>,
    params: MultimodeParams,
}

#[derive(Default)]
struct MultimodeState {
    sample_rate: f32,
    /// Channel count the groups' lanes were last set up for.
    channels: usize,
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
fn mode_gains(mode: f32) -> (vca::Gain, vca::Gain) {
    let gain = |offset: f32| vca::Gain::new((mode * MODE_SCALE - offset).clamp(0.0, 1.0));
    (gain(MODE_OFFSET_A), gain(MODE_OFFSET_B))
}

/// The mode VCAs' mix into the output amp, in volts across R173. VCA A takes
/// BP and HP through 62k each; VCA B takes its output less BP and LP (through
/// the IC14C summer) and adds to the LP that reaches the output directly.
#[inline]
fn morph<L: Lanes>(lp: L, bp: L, hp: L, (gain_a, gain_b): &(GainX4, GainX4)) -> L {
    // Below mode 2.5 VCA A is off, and at mode 0 VCA B too, so LP reaches the
    // output alone.
    let live = |gain: &GainX4| L::bits(gain.gain::<L>().ge(L::splat(f32::MIN_POSITIVE))) != 0;
    let (r, inv_r) = (L::splat(R_MODE_IN), L::splat(1.0 / R_MODE_IN));
    let a = if live(gain_a) {
        r * vca::output(gain_a, (bp + hp) * inv_r)
    } else {
        L::splat(0.0)
    };
    if !live(gain_b) {
        return lp;
    }
    let w = a - bp - lp;
    lp + r * vca::output(gain_b, w * inv_r)
}

/// `read(channel)` for the first `active` channels from `base`, 0 for dead
/// lanes.
#[inline]
fn lanes(base: usize, active: usize, read: impl Fn(usize) -> f32) -> [f32; LANES] {
    std::array::from_fn(|lane| {
        if lane < active {
            read(base + lane)
        } else {
            0.0
        }
    })
}

impl Multimode {
    fn init(&mut self, sample_rate: f32) {
        components::prime();
        drive::prime();
        output::prime();
        self.state.sample_rate = sample_rate;
        let base = seed_base(self);
        for (group, state) in self.channel_state.iter_mut().enumerate() {
            for (lane, noise) in state.noise.iter_mut().enumerate() {
                noise.seed(base, group * LANES + lane);
            }
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
        let channels = self.channel_count();
        let rate = self.core_rate();
        let tables = Tables::get();
        let (p, outputs) = (&self.params, &mut self.outputs);
        // A single channel runs on lane 0 alone, without three dead lanes.
        if channels == 1 {
            process::<f32>(&mut self.channel_state[0], p, outputs, 0, 1, rate, tables);
            return;
        }
        for (group, state) in self
            .channel_state
            .iter_mut()
            .take(channels.div_ceil(LANES))
            .enumerate()
        {
            let base = group * LANES;
            let active = (channels - base).min(LANES);
            // A group's one live channel runs on lane 0 as well.
            if active == 1 {
                process_group::<f32>(state, p, outputs, base, active, rate, tables);
            } else {
                process_group::<f32x4>(state, p, outputs, base, active, rate, tables);
            }
        }
    }
}

/// [`process`] kept out of line, so `update` carries one inlined copy.
#[inline(never)]
fn process_group<L: Lanes>(
    state: &mut GroupState,
    p: &MultimodeParams,
    outputs: &mut MultimodeOutputs,
    base: usize,
    active: usize,
    rate: f32,
    tables: Tables,
) {
    process::<L>(state, p, outputs, base, active, rate, tables);
}

/// One sample for the `active` channels of group `state`, from channel `base`.
#[inline(always)]
fn process<L: Lanes>(
    state: &mut GroupState,
    p: &MultimodeParams,
    outputs: &mut MultimodeOutputs,
    base: usize,
    active: usize,
    rate: f32,
    tables: Tables,
) {
    let read = |signal: &Option<PolySignal>| lanes(base, active, |ch| signal.value_or(ch, 0.0));

    let c = &mut state.controls;
    let dead = active.min(L::WIDTH)..L::WIDTH;
    // Each control law sanitizes its input, so a non-finite control acts
    // as 0 V rather than reaching the filter state.
    c.drive.update(
        read(&p.drive),
        active,
        dead.clone(),
        |drive| {
            let servo = (1.0 + sanitize(drive) * DRIVE_SERVO_SCALE).max(0.0);
            vca::Gain::new(servo.powf(DRIVE_EXPONENT).min(DRIVE_MAX_GAIN))
        },
        |lane, g| c.drive_x4.set(lane, g),
    );
    c.fold.update(
        read(&p.fold),
        active,
        dead.clone(),
        |fold| fold_gains(sanitize(fold)),
        |lane, g| c.fold_gains[lane] = *g,
    );
    let fold = c.fold_gains;
    c.tuning.update(
        lanes(base, active, |ch| p.cutoff.get_value(ch)),
        active,
        dead.clone(),
        |voct| {
            let fc = voct_to_hz(sanitize(voct)).clamp(FC_MIN_HZ, FC_MAX_HZ);
            CoreTuning::new(fc, fc / FC_MAX_HZ, rate)
        },
        |lane, t| c.tuning_x4.set(lane, t),
    );
    c.res.update(
        read(&p.resonance),
        active,
        dead.clone(),
        |resonance| vca::Gain::new(vca_gain((sanitize(resonance) * RES_CONTROL_SCALE).max(0.0))),
        |lane, g| c.res_x4.set(lane, g),
    );
    c.mode.update(
        read(&p.mode),
        active,
        dead,
        |mode| mode_gains(sanitize(mode)),
        |lane, (a, b)| {
            c.mode_x4.0.set(lane, a);
            c.mode_x4.1.set(lane, b);
        },
    );

    let input = L::from_lanes(lanes(base, active, |ch| p.input.get_value(ch)));
    let drive_in = input * L::splat(1.0 / R_DRIVE_IN);
    let x = (L::splat(R_DRIVE_IN) * vca::output(&c.drive_x4, drive_in)).sanitize();
    let out = if p.high_res {
        let (even, odd) = state.up.process(x);
        let even = state.tick(even, &fold, tables, active);
        let odd = state.tick(odd, &fold, tables, active);
        // A lone channel decimates its four outputs as one downsampler's lanes.
        if L::WIDTH == 1 {
            let pack = |o: [L; 4]| f32x4::new(o.map(|v| v.to_lanes()[0]));
            let y = state.down[0].process(pack(even), pack(odd)).to_array();
            y.map(L::splat)
        } else {
            std::array::from_fn(|k| state.down[k].process(even[k], odd[k]))
        }
    } else {
        let x = state.pre_emphasis.process(x);
        state.tick(x, &fold, tables, active)
    };

    let [sample, lp, bp, hp] = out.map(|v| rails(v).to_lanes());
    for lane in 0..active {
        let ch = base + lane;
        outputs.sample.set(ch, sample[lane]);
        outputs.lp.set(ch, lp[lane]);
        outputs.bp.set(ch, bp[lane]);
        outputs.hp.set(ch, hp[lane]);
    }
}

impl crate::types::PatchUpdateHandler for Multimode {
    fn on_patch_update(&mut self) {
        let rate = self.core_rate();
        for group in self.channel_state.iter_mut() {
            group.configure(rate);
        }
        let (old, channels) = (self.state.channels, self.channel_count());
        // A group with one live channel keeps that channel's four output
        // downsamplers in one downsampler's lanes; a group moving between one
        // live channel and several transposes them.
        let live = |count: usize, group: usize| count.saturating_sub(group * LANES).min(LANES);
        for (group, state) in self.channel_state.iter_mut().enumerate() {
            let (before, after) = (live(old, group), live(channels, group));
            if before != 0 && after != 0 && (before == 1) != (after == 1) {
                HalfbandDown::transpose(&mut state.down);
            }
        }
        // Channels new to this patch start from rest, as they would in a fresh
        // module, rather than from a lane that ran dead before.
        for ch in old.min(channels)..channels {
            self.channel_state[ch / LANES].clear_lane(ch % LANES);
        }
        // A new channel count changes which lanes are dead; recomputing every
        // law sets them to follow lane 0 again.
        if old != channels {
            for group in self.channel_state.iter_mut() {
                group.controls.clear();
            }
        }
        self.state.channels = channels;
    }
}

message_handlers!(impl Multimode {});

#[cfg(test)]
mod tests;
