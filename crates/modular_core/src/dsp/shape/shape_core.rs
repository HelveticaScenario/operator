//! Shared engine for the `$unstable.shape.*` waveshaper family.
//!
//! Every `$unstable.shape.*` module is the same machine — smooth the drive, normalize the
//! signal into the shaper's `[-1, 1]` domain, optionally 2× oversample, apply a
//! per-`mode` nonlinearity, fade in the onset, and scale back to modular level —
//! differing only in the set of nonlinearities (the [`Shaper`] impl). Channels
//! are processed one at a time in `f32`.

use crate::dsp::utils::dc_blocker::{DEFAULT_DC_BLOCK_FC_HZ, DcBlocker};
use crate::dsp::utils::fade::{DEFAULT_FADE_IN_SECS, FadeIn};
use crate::dsp::utils::halfband::{Halfband2xDown, Halfband2xUp};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use crate::types::Clickless;

/// Modular audio level. Signals swing ±5 V; the Surge XT shapers work in `[-1, 1]`,
/// so we divide going in and multiply coming out.
pub(crate) const AUDIO_LEVEL: f32 = 5.0;

/// Decibels per unit of the −5..5 `drive` parameter: ∓12 dB at the extremes.
/// Surge XT's own drive knob spans ∓24 dB; ∓12 dB is a gentler modular range
/// over the same true-dB curve (+12 dB ≈ ×4 at the top instead of ×16).
const DRIVE_DB_PER_UNIT: f32 = 12.0 / 5.0;

/// Map the smoothed, clamped `drive` parameter to a linear gain using Surge XT's
/// `db_to_linear` curve — true dB, `10^(dB/20)` (0 = unity, +6 dB ≈ ×2).
#[inline]
fn drive_map(drive_param: f32) -> f32 {
    let db = drive_param.clamp(-5.0, 5.0) * DRIVE_DB_PER_UNIT;
    (db * (std::f32::consts::LOG2_10 / 20.0)).exp2()
}

/// A family of nonlinearities selected by `Mode`, plus their per-mode state.
///
/// `shape` receives the ±1-normalized, 2×-oversampled input and the drive
/// multiplier and returns the shaped ±1 value. It takes `&mut self` so stateful
/// shapers (ADAA, DC blockers) can advance; it is called twice per input sample
/// (even/odd), so their state runs at the oversampled rate — matching Surge XT,
/// which runs the waveshaper inside its 2×-oversampled filter block.
pub trait Shaper: Copy + Default {
    type Mode: Copy;

    fn shape(&mut self, x: f32, drive: f32, mode: Self::Mode, dc_coeff: f32) -> f32;

    /// One-time, main-thread setup (called from `init`). Shapers backed by lazily
    /// built tables force them here so the fill never runs on the audio thread.
    fn prime() {}
}

/// Per-channel runtime state: drive smoother, onset fade, oversampling pair, and
/// the shaper's own state. One instance per channel.
#[derive(Clone, Copy, Default)]
pub struct ShapeChannel<S: Shaper> {
    drive: Clickless,
    fade: FadeIn,
    up: Halfband2xUp,
    down: Halfband2xDown,
    shaper: S,
}

impl<S: Shaper> ShapeChannel<S> {
    #[inline]
    fn process(
        &mut self,
        x_in: f32,
        drive_raw: f32,
        mode: S::Mode,
        dc_coeff: f32,
        fade_inc: f32,
    ) -> f32 {
        self.drive.update(drive_raw);
        let drive = drive_map(*self.drive);
        let x = x_in / AUDIO_LEVEL;

        // Always 2× oversample: Surge XT runs the waveshaper inside its filter
        // block at 2×, so every mode — including the ADAA folds/rectifiers —
        // matches that by running its state at the oversampled rate.
        let (even, odd) = self.up.process(x);
        let se = self.shaper.shape(even, drive, mode, dc_coeff);
        let so = self.shaper.shape(odd, drive, mode, dc_coeff);
        let y = self.down.process(se, so);

        y * AUDIO_LEVEL * self.fade.advance(fade_inc)
    }
}

/// Sample-rate-derived constants shared by every `$unstable.shape.*` module. Seeded in
/// `init` (sample-rate-only, so it survives the `state` swap on a patch update).
#[derive(Default)]
pub struct ShapeModuleState {
    /// DC-blocker feedback coefficient for a fixed corner frequency at this
    /// sample rate (unlike Surge XT's fixed 0.9999, whose cutoff drifts with rate).
    pub(crate) dc_coeff: f32,
    pub(crate) fade_inc: f32,
}

impl ShapeModuleState {
    #[inline]
    pub fn init(&mut self, sample_rate: f32) {
        self.dc_coeff = DcBlocker::coeff(DEFAULT_DC_BLOCK_FC_HZ, sample_rate);
        self.fade_inc = FadeIn::increment(DEFAULT_FADE_IN_SECS, sample_rate);
    }
}

/// Run one sample across all channels of a `$unstable.shape.*` module: gather, shape,
/// scatter. This is the entire per-sample body every module shares.
#[inline]
pub fn run<S: Shaper>(
    channels: usize,
    input: &PolySignal,
    drive: &Option<PolySignal>,
    mode: S::Mode,
    state: &ShapeModuleState,
    channel_state: &mut [ShapeChannel<S>],
    out: &mut PolyOutput,
) {
    for ch in 0..channels {
        let x = input.get_value(ch);
        let d = drive.value_or(ch, 0.0);
        let y = channel_state[ch].process(x, d, mode, state.dc_coeff, state.fade_inc);
        out.set(ch, y);
    }
}
