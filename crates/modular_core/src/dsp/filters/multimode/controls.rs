//! Control laws: each control voltage mapped to the gain or tuning its stage
//! reads, recomputed per lane only when the control changes.

use crate::{
    block_input::BlockInput,
    dsp::utils::{sanitize, voct_to_hz},
    poly::{PolySignal, PolySignalExt},
};

use super::lanes::{LANES, LaneCache, Lanes};
use super::svf::{CoreTuning, TuningX4};
use super::vca::{self, GainX4};
use super::{MultimodeParams, MultimodeParamsBlockInputs, lanes};

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
pub(super) const DRIVE_EXPONENT: f32 = 3.02;
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

/// Control laws per lane, each stored in the form its stage reads and
/// recomputed only for lanes whose control changed. The integrator gains also
/// depend on the core rate; [`GroupState::configure`](super::group::GroupState::configure) clears them when it
/// changes.
#[derive(Default, Clone, Copy)]
pub(super) struct Controls {
    pub(super) drive: LaneCache,
    pub(super) drive_x4: GainX4,
    pub(super) fold: LaneCache,
    pub(super) fold_gains: [(f32, f32); LANES],
    pub(super) tuning: LaneCache,
    pub(super) tuning_x4: TuningX4,
    pub(super) res: LaneCache,
    pub(super) res_x4: GainX4,
    pub(super) mode: LaneCache,
    pub(super) mode_x4: (GainX4, GainX4),
}

impl Controls {
    pub(super) fn clear(&mut self) {
        self.drive.clear();
        self.fold.clear();
        self.tuning.clear();
        self.res.clear();
        self.mode.clear();
    }
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

/// One sample's control inputs for a group's live channels, 0 in dead lanes.
pub(super) struct ControlInputs {
    cutoff: [f32; LANES],
    resonance: [f32; LANES],
    mode: [f32; LANES],
    drive: [f32; LANES],
    fold: [f32; LANES],
}

impl ControlInputs {
    /// The current sample's controls for the `active` channels from `base`.
    #[inline(always)]
    pub(super) fn read(p: &MultimodeParams, base: usize, active: usize) -> Self {
        let option = |signal: &Option<PolySignal>, default: f32| {
            lanes(base, active, |ch| signal.value_or(ch, default))
        };
        Self {
            cutoff: lanes(base, active, |ch| p.cutoff.get_value(ch)),
            resonance: option(&p.resonance, MultimodeParams::RESONANCE_DEFAULT),
            mode: option(&p.mode, MultimodeParams::MODE_DEFAULT),
            drive: option(&p.drive, MultimodeParams::DRIVE_DEFAULT),
            fold: option(&p.fold, MultimodeParams::FOLD_DEFAULT),
        }
    }

    /// Slot `i`'s controls for the `active` channels from `base`.
    #[inline(always)]
    pub(super) fn row(
        inputs: &MultimodeParamsBlockInputs,
        i: usize,
        base: usize,
        active: usize,
    ) -> Self {
        Self {
            cutoff: row(&inputs.cutoff, i, base, active),
            resonance: row(&inputs.resonance, i, base, active),
            mode: row(&inputs.mode, i, base, active),
            drive: row(&inputs.drive, i, base, active),
            fold: row(&inputs.fold, i, base, active),
        }
    }
}

/// Slot `i` of `input` for the `active` channels from `base`, 0 for dead
/// lanes.
#[inline(always)]
pub(super) fn row(input: &BlockInput, i: usize, base: usize, active: usize) -> [f32; LANES] {
    let row = &input.row(i)[base..base + active];
    lanes(0, active, |lane| row[lane])
}

/// Recomputes the control laws of the first `active` lanes whose controls
/// changed.
#[inline(always)]
pub(super) fn update_controls<L: Lanes>(
    c: &mut Controls,
    inputs: &ControlInputs,
    active: usize,
    rate: f32,
) {
    let dead = active.min(L::WIDTH)..L::WIDTH;
    // Each control law sanitizes its input, so a non-finite control acts
    // as 0 V rather than reaching the filter state.
    c.drive.update(
        inputs.drive,
        active,
        dead.clone(),
        |drive| {
            let servo = (1.0 + sanitize(drive) * DRIVE_SERVO_SCALE).max(0.0);
            vca::Gain::new(servo.powf(DRIVE_EXPONENT).min(DRIVE_MAX_GAIN))
        },
        |lane, g| c.drive_x4.set(lane, g),
    );
    c.fold.update(
        inputs.fold,
        active,
        dead.clone(),
        |fold| fold_gains(sanitize(fold)),
        |lane, g| c.fold_gains[lane] = *g,
    );
    c.tuning.update(
        inputs.cutoff,
        active,
        dead.clone(),
        |voct| {
            let fc = voct_to_hz(sanitize(voct)).clamp(FC_MIN_HZ, FC_MAX_HZ);
            CoreTuning::new(fc, fc / FC_MAX_HZ, rate)
        },
        |lane, t| c.tuning_x4.set(lane, t),
    );
    c.res.update(
        inputs.resonance,
        active,
        dead.clone(),
        |resonance| vca::Gain::new(vca_gain((sanitize(resonance) * RES_CONTROL_SCALE).max(0.0))),
        |lane, g| c.res_x4.set(lane, g),
    );
    c.mode.update(
        inputs.mode,
        active,
        dead,
        |mode| mode_gains(sanitize(mode)),
        |lane, (a, b)| {
            c.mode_x4.0.set(lane, a);
            c.mode_x4.1.set(lane, b);
        },
    );
}
