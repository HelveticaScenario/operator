//! Shared engine for the `$unstable.filter.*` Surge filter family.
//!
//! Every module is the same machine — normalize the ±5 V input into the filter's
//! ±1 domain, run one Surge `FilterType` (a `mode` enum selects passband / slope /
//! drive), and scale back — differing only in the DSP kernel (the [`Filter`] impl).
//! Channels are processed one at a time in `f32`.
//!
//! Coefficients are evaluated on a change-gated control-rate tick rather than every
//! sample: at most once per [`COEFF_TICK`] samples, and only when the control input
//! moved, the raw target `N` is rebuilt; every tick a cheap glide eases the live
//! coefficients `C` toward `N` (Surge XT's `FromDirect` smoothing), and each sample the
//! `process` kernel advances `C += dC`. Modes with a per-sample saturator opt into 2×
//! oversampling via [`Filter::oversample`].
//!
//! Changing a non-signal param (drive / slope / mode) switches the coefficient *form*
//! — the `C[]`/`R[]` slots then mean something different, so neither the glide nor the
//! carried-over state is valid. A form change instead crossfades: the old form keeps
//! running with frozen coefficients while the new form restarts from rest, blended over
//! [`FADE_SAMPLES`], so the transition is click-free.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use crate::dsp::utils::changed;
use crate::dsp::utils::halfband::{Halfband2xDown, Halfband2xUp};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// Coefficient slots per filter (Surge XT's `n_cm_coeffs`).
pub const N_COEFFS: usize = 8;
/// Filter state registers (Surge XT's `n_filter_registers`).
pub const N_REGISTERS: usize = 16;

/// Modular audio level: ±5 V signals are divided to ±1 for the Surge kernels
/// (whose in-loop saturators are tuned for that range) and multiplied back out.
pub const AUDIO_LEVEL: f32 = 5.0;

/// Control-rate: coefficients are re-evaluated/glided at most once every this many
/// samples. Fixed, independent of the audio block size.
const COEFF_TICK: u32 = 16;

/// Surge XT's `FromDirect` target-smoothing constant (one-pole glide of the target).
const SMOOTH: f32 = 0.2;

/// Crossfade length for a coefficient-form change (~5 ms at 48 kHz).
const FADE_SAMPLES: u32 = 256;

/// Zero coefficient deltas — the crossfading old filter runs with frozen coefficients.
const ZERO_DC: [f32; N_COEFFS] = [0.0; N_COEFFS];

/// V/Oct (0 V = C4, MIDI 60) → semitones relative to A440 (MIDI 69), the pitch unit
/// Surge XT's coefficient maker expects (`440 · 2^(semi/12)` Hz).
#[inline]
pub fn voct_to_semitones_a440(voct: f32) -> f32 {
    voct * 12.0 - 9.0
}

/// A Surge filter kernel. `Mode` selects the concrete `FilterType` + subtype; the
/// kernel builds raw target coefficients and processes one sample.
pub trait Filter: Copy + Default {
    type Mode: Copy + PartialEq;

    /// Extra per-channel state beyond the register block — the comb's delay line.
    /// `()` for kernels whose state fits the registers. Constructed by `Default`
    /// during module construction (main thread, so it may allocate).
    type Extra: Default;

    /// Build the raw target coefficients `N` for `mode` at `freq_semi` (semitones
    /// from A440), `reso` (0..1), running at `rate` Hz (2×base when oversampled).
    fn coeffs(mode: Self::Mode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS];

    /// Process one sample. `c` are the live coefficients (advanced by `dc` inside,
    /// matching Surge — some kernels read `c` both pre- and post-advance), `r` the
    /// state registers, `extra` the kernel's [`Filter::Extra`] state. Returns the
    /// ±1-domain output.
    fn process(
        mode: Self::Mode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        extra: &mut Self::Extra,
    ) -> f32;

    /// Whether `mode`'s `process` applies a hard per-sample saturator and so warrants
    /// 2× oversampling. Linear kernels return `false` (the default).
    fn oversample(_mode: Self::Mode) -> bool {
        false
    }

    /// Whether a mode change swaps the coefficient *form* and so needs the
    /// crossfade. Kernels whose modes only re-target coefficients in place (the
    /// comb's polarity/mix) return `false` — the glide alone is click-free, and the
    /// crossfade cannot snapshot a non-unit [`Filter::Extra`] (both forms would
    /// share it).
    fn crossfade_on_mode_change() -> bool {
        true
    }
}

/// Per-channel runtime state: the coefficient glide (`C`/`tC`/`dC`/`N`), the change
/// gate (`last_cutoff`/`last_reso`), the state registers `R`, the oversampling pair,
/// the kernel's [`Filter::Extra`] state, and a frozen snapshot of the previous form
/// used while crossfading.
#[derive(Clone)]
pub struct FilterChannel<E = ()> {
    c: [f32; N_COEFFS],
    tc: [f32; N_COEFFS],
    dc: [f32; N_COEFFS],
    n: [f32; N_COEFFS],
    r: [f32; N_REGISTERS],
    last_cutoff: f32,
    last_reso: f32,
    first_run: bool,
    up: Halfband2xUp,
    down: Halfband2xDown,
    extra: E,
    /// Frozen previous-form coefficients + state, run to produce the fade-out signal.
    fade_c: [f32; N_COEFFS],
    fade_r: [f32; N_REGISTERS],
    fade_up: Halfband2xUp,
    fade_down: Halfband2xDown,
}

impl<E: Default> Default for FilterChannel<E> {
    fn default() -> Self {
        Self {
            c: [0.0; N_COEFFS],
            tc: [0.0; N_COEFFS],
            dc: [0.0; N_COEFFS],
            n: [0.0; N_COEFFS],
            r: [0.0; N_REGISTERS],
            last_cutoff: f32::NAN,
            last_reso: f32::NAN,
            first_run: true,
            up: Halfband2xUp::default(),
            down: Halfband2xDown::default(),
            extra: E::default(),
            fade_c: [0.0; N_COEFFS],
            fade_r: [0.0; N_REGISTERS],
            fade_up: Halfband2xUp::default(),
            fade_down: Halfband2xDown::default(),
        }
    }
}

impl<E> FilterChannel<E> {
    /// Ease the live coefficients toward the raw target `n` (Surge XT's `FromDirect`).
    /// `steps` is how many `C += dC` advances happen before the next glide — one per
    /// `process` call between ticks — so the ramp lands on `tC` exactly at the tick.
    #[inline]
    fn glide(&mut self, steps: f32) {
        if self.first_run {
            self.c = self.n;
            self.tc = self.n;
            self.dc = [0.0; N_COEFFS];
            self.first_run = false;
        } else {
            for i in 0..N_COEFFS {
                self.tc[i] = (1.0 - SMOOTH) * self.tc[i] + SMOOTH * self.n[i];
                self.dc[i] = (self.tc[i] - self.c[i]) / steps;
            }
        }
    }

    /// Snapshot the current (old-form) coefficients + state into the fade slot, then
    /// restart the live filter from rest so the new form settles with no jump. The
    /// live coefficients are rebuilt+snapped by the next tick (`first_run`).
    #[inline]
    fn begin_fade(&mut self) {
        self.fade_c = self.c;
        self.fade_r = self.r;
        self.fade_up = self.up;
        self.fade_down = self.down;
        self.r = [0.0; N_REGISTERS];
        self.up = Halfband2xUp::default();
        self.down = Halfband2xDown::default();
        self.first_run = true;
        self.last_cutoff = f32::NAN;
        self.last_reso = f32::NAN;
    }
}

/// Module-level state: the control-rate tick phase, the last-applied mode (to detect a
/// form change on patch update), and the crossfade countdown. Survives the `state`
/// swap on a patch update.
pub struct FilterModuleState<M> {
    tick: u32,
    last_mode: Option<M>,
    fade: u32,
    fade_mode: Option<M>,
}

impl<M> Default for FilterModuleState<M> {
    fn default() -> Self {
        Self {
            tick: 0,
            last_mode: None,
            fade: 0,
            fade_mode: None,
        }
    }
}

/// Called from a module's `on_patch_update` with the mode derived from the new params.
/// If the coefficient form changed, start a click-free crossfade from the old form to
/// the new one (or, for kernels that opt out, force a coefficient rebuild so the glide
/// carries the change); otherwise a cutoff/reso change is handled by the normal glide.
#[inline]
pub fn on_patch_update<F: Filter>(
    state: &mut FilterModuleState<F::Mode>,
    channel_state: &mut [FilterChannel<F::Extra>],
    mode: F::Mode,
) {
    if let Some(old) = state.last_mode {
        if old != mode {
            if F::crossfade_on_mode_change() {
                state.fade_mode = Some(old);
                state.fade = FADE_SAMPLES;
                for cs in channel_state.iter_mut() {
                    cs.begin_fade();
                }
            } else {
                // Re-fire the change gate; the glide eases the coefficients to the
                // new mode's targets.
                for cs in channel_state.iter_mut() {
                    cs.last_cutoff = f32::NAN;
                    cs.last_reso = f32::NAN;
                }
            }
        }
    }
    state.last_mode = Some(mode);
    state.tick = 0;
}

/// Run one filter kernel for one input sample, oversampling 2× when `oversample`.
#[inline]
#[allow(clippy::too_many_arguments)]
fn process_sample<F: Filter>(
    mode: F::Mode,
    x: f32,
    oversample: bool,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
    extra: &mut F::Extra,
    up: &mut Halfband2xUp,
    down: &mut Halfband2xDown,
) -> f32 {
    if oversample {
        let (even, odd) = up.process(x);
        let se = F::process(mode, even, c, dc, r, extra);
        let so = F::process(mode, odd, c, dc, r, extra);
        down.process(se, so)
    } else {
        F::process(mode, x, c, dc, r, extra)
    }
}

/// Run one sample across all channels of a `$unstable.filter.*` module: on the tick,
/// change-gate + glide each channel's coefficients; every sample, normalize, run the
/// kernel (optionally 2× oversampled, optionally crossfading a form change), and write
/// the output.
#[inline]
pub fn run<F: Filter>(
    channels: usize,
    input: &PolySignal,
    cutoff: &PolySignal,
    resonance: &Option<PolySignal>,
    mode: F::Mode,
    sample_rate: f32,
    state: &mut FilterModuleState<F::Mode>,
    channel_state: &mut [FilterChannel<F::Extra>],
    out: &mut PolyOutput,
) {
    let oversample = F::oversample(mode);
    let rate = if oversample {
        sample_rate * 2.0
    } else {
        sample_rate
    };
    // `C += dC` runs once per `process` call: `oversample` gives two calls per input
    // sample, so the ramp spans `COEFF_TICK * os` advances between glides.
    let steps = COEFF_TICK as f32 * if oversample { 2.0 } else { 1.0 };

    if state.tick == 0 {
        for ch in 0..channels {
            let cs = &mut channel_state[ch];
            let cutoff_v = cutoff.get_value(ch);
            let reso = (resonance.value_or(ch, 0.0) / 5.0).clamp(0.0, 1.0);
            if changed(cutoff_v, cs.last_cutoff) || changed(reso, cs.last_reso) {
                let freq_semi = voct_to_semitones_a440(cutoff_v);
                cs.n = F::coeffs(mode, freq_semi, reso, rate);
                cs.last_cutoff = cutoff_v;
                cs.last_reso = reso;
            }
            cs.glide(steps);
        }
    }

    let fading = state.fade > 0;
    let fade_mode = state.fade_mode;
    let fade_oversample = fade_mode.map(F::oversample).unwrap_or(false);
    // New-form weight this sample: ~0 at the start of the fade, ~1 at the end.
    let t = if fading {
        (1.0 - (state.fade as f32 - 1.0) / FADE_SAMPLES as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let inv_level = 1.0 / AUDIO_LEVEL;
    for ch in 0..channels {
        let cs = &mut channel_state[ch];
        let x = input.get_value(ch) * inv_level;
        let new_y = process_sample::<F>(
            mode,
            x,
            oversample,
            &mut cs.c,
            &cs.dc,
            &mut cs.r,
            &mut cs.extra,
            &mut cs.up,
            &mut cs.down,
        );
        let y = if let (true, Some(fm)) = (fading, fade_mode) {
            // The fade shares the live `extra` — only kernels with a unit `Extra`
            // reach here (others opt out via `crossfade_on_mode_change`).
            let old_y = process_sample::<F>(
                fm,
                x,
                fade_oversample,
                &mut cs.fade_c,
                &ZERO_DC,
                &mut cs.fade_r,
                &mut cs.extra,
                &mut cs.fade_up,
                &mut cs.fade_down,
            );
            old_y * (1.0 - t) + new_y * t
        } else {
            new_y
        };
        out.set(ch, y * AUDIO_LEVEL);
    }

    if fading {
        state.fade -= 1;
        if state.fade == 0 {
            state.fade_mode = None;
        }
    }

    state.tick += 1;
    if state.tick >= COEFF_TICK {
        state.tick = 0;
    }
}

#[cfg(test)]
pub mod test_util {
    use super::*;
    use crate::types::Signal;

    /// Steady-state RMS of a `freq_hz` unit sine driven through the settled kernel
    /// (coefficients built once, no glide). Asserts finite, bounded output throughout.
    pub fn sine_rms<F: Filter>(
        mode: F::Mode,
        cutoff_semi: f32,
        reso: f32,
        freq_hz: f32,
        sr: f32,
    ) -> f32 {
        sine_rms_amp::<F>(mode, cutoff_semi, reso, freq_hz, sr, 1.0)
    }

    /// [`sine_rms`] with an input amplitude — probe level-dependent kernels in their
    /// small-signal (linear) regime to verify the underlying response shape.
    pub fn sine_rms_amp<F: Filter>(
        mode: F::Mode,
        cutoff_semi: f32,
        reso: f32,
        freq_hz: f32,
        sr: f32,
        amp: f32,
    ) -> f32 {
        let mut c = F::coeffs(mode, cutoff_semi, reso, sr);
        let dc = [0.0f32; N_COEFFS];
        let mut r = [0.0f32; N_REGISTERS];
        let mut extra = F::Extra::default();
        let n = 16_000usize;
        let mut sumsq = 0.0f64;
        let mut counted = 0u32;
        for i in 0..n {
            let t = i as f32 / sr;
            let x = (2.0 * std::f32::consts::PI * freq_hz * t).sin() * amp;
            let y = F::process(mode, x, &mut c, &dc, &mut r, &mut extra);
            assert!(y.is_finite() && y.abs() < 8.0, "unbounded output {y}");
            if i > n / 2 {
                sumsq += (y as f64) * (y as f64);
                counted += 1;
            }
        }
        (sumsq / counted as f64).sqrt() as f32
    }

    /// Drive every `mode` through the full engine (`run`) for one second with a
    /// full-range cutoff sweep at maximum resonance, asserting the output stays
    /// finite and bounded — exercises the change gate, glide, and tick machinery.
    pub fn sweep_stays_bounded<F: Filter>(modes: &[F::Mode])
    where
        F::Mode: std::fmt::Debug,
    {
        let sr = 48_000.0;
        let n = 48_000usize;
        for &mode in modes {
            let mut state = FilterModuleState::default();
            let mut ch = vec![FilterChannel::<F::Extra>::default()];
            let mut out = PolyOutput::mono(0.0);
            on_patch_update::<F>(&mut state, &mut ch, mode);
            let reso = Some(PolySignal::mono(Signal::Volts(5.0)));
            for i in 0..n {
                let t = i as f32 / sr;
                let saw = ((t * 220.0).fract() * 2.0 - 1.0) * 5.0;
                let input = PolySignal::mono(Signal::Volts(saw));
                let cutoff_v = -5.0 + 10.0 * (i as f32 / n as f32);
                let cutoff = PolySignal::mono(Signal::Volts(cutoff_v));
                run::<F>(
                    1, &input, &cutoff, &reso, mode, sr, &mut state, &mut ch, &mut out,
                );
                let y = out.get(0);
                // Loose bound: a linear high-Q peak legitimately rings to hundreds of
                // volts at max resonance; genuine divergence still lands at inf/NaN.
                assert!(
                    y.is_finite() && y.abs() < 500.0,
                    "unbounded output {y} at sample {i} (mode {mode:?})"
                );
            }
        }
    }
}
