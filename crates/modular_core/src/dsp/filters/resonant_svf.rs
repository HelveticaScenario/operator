//! Shared engine for `$lpf`, `$hpf` and `$bpf`: a trapezoidal SVF whose
//! cutoff and resonance glide only across patch updates, so live modulation
//! reaches the filter unsmoothed.

use crate::dsp::filters::svf_core::{self, SvfCoeffs};
use crate::dsp::utils::{changed, sanitize, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use crate::types::SwapGlide;

#[derive(Clone, Copy)]
pub enum Response {
    Lowpass,
    Highpass,
    /// Unity gain at the center frequency.
    Bandpass,
}

impl Response {
    /// The SVF output mix for this response at integrator gain `g` and damping `k`.
    pub(crate) fn coeffs(self, g: f32, k: f32) -> SvfCoeffs {
        match self {
            Response::Lowpass => SvfCoeffs::new(g, k, 0.0, 0.0, 1.0),
            Response::Highpass => SvfCoeffs::new(g, k, 1.0, -k, -1.0),
            Response::Bandpass => SvfCoeffs::new(g, k, 0.0, k, 0.0),
        }
    }
}

/// The SVF integrator gain for a cutoff in V/Oct, held between 20 Hz and
/// 0.45 × the sample rate.
pub(crate) fn cutoff_gain(cutoff: f32, sample_rate: f32) -> f32 {
    let freq = voct_to_hz(cutoff).min(sample_rate * 0.45).max(20.0);
    (std::f32::consts::PI * freq / sample_rate).tan()
}

fn compute_coeffs(response: Response, cutoff: f32, resonance: f32, sample_rate: f32) -> SvfCoeffs {
    let k = 1.0 / (resonance / 5.0 * 9.0 + 0.5).max(0.5);
    response.coeffs(cutoff_gain(cutoff, sample_rate), k)
}

/// A filter's glided cutoff and resonance.
#[derive(Clone, Copy, Default)]
pub(crate) struct CutoffResonance {
    cutoff: SwapGlide,
    resonance: SwapGlide,
}

impl CutoffResonance {
    /// Glides both params at channel `ch` and returns their current values.
    #[inline]
    pub(crate) fn update(
        &mut self,
        cutoff: &PolySignal,
        resonance: &Option<PolySignal>,
        default_resonance: f32,
        ch: usize,
        sample_rate: f32,
    ) -> (f32, f32) {
        self.cutoff
            .update(cutoff.get_value(ch), cutoff.glide_mark(ch), sample_rate);
        self.resonance.update(
            resonance.value_or(ch, default_resonance),
            resonance.glide_mark(ch),
            sample_rate,
        );
        (*self.cutoff, *self.resonance)
    }
}

/// Whether cutoff and resonance are both monophonic, so every channel can
/// share one set of coefficients.
pub(crate) fn params_mono(cutoff: &PolySignal, resonance: &Option<PolySignal>) -> bool {
    cutoff.is_monophonic() && resonance.as_ref().is_none_or(|s| s.is_monophonic())
}

/// When an edit switches between the shared and per-channel coefficient
/// paths, the newly active path takes over the other's glide state, so it
/// continues from what was actually playing.
pub(crate) fn hand_over<'a>(
    mono: bool,
    was_mono: bool,
    shared: &mut CutoffResonance,
    mut per_channel: impl Iterator<Item = &'a mut CutoffResonance>,
) {
    if mono && !was_mono {
        if let Some(first) = per_channel.next() {
            *shared = *first;
        }
    } else if !mono && was_mono {
        for glides in per_channel {
            *glides = *shared;
        }
    }
}

/// Glided params and the coefficients derived from them. `NaN` = never computed.
#[derive(Clone, Copy)]
pub struct CoeffCache {
    coeffs: SvfCoeffs,
    last_cutoff: f32,
    last_resonance: f32,
    glides: CutoffResonance,
}

impl Default for CoeffCache {
    fn default() -> Self {
        Self {
            coeffs: SvfCoeffs::default(),
            last_cutoff: f32::NAN,
            last_resonance: f32::NAN,
            glides: CutoffResonance::default(),
        }
    }
}

impl CoeffCache {
    #[inline]
    fn refresh(
        &mut self,
        response: Response,
        cutoff: &PolySignal,
        resonance: &Option<PolySignal>,
        default_resonance: f32,
        ch: usize,
        sample_rate: f32,
    ) {
        let (c, r) = self
            .glides
            .update(cutoff, resonance, default_resonance, ch, sample_rate);
        if changed(c, self.last_cutoff) || changed(r, self.last_resonance) {
            self.coeffs = compute_coeffs(response, c, r, sample_rate);
            self.last_cutoff = c;
            self.last_resonance = r;
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct SvfChannel {
    ic1: f32,
    ic2: f32,
    cache: CoeffCache,
}

/// Module-level state: the shared coefficients used when cutoff and
/// resonance are both monophonic.
#[derive(Default)]
pub struct SvfFilterState {
    mono: CoeffCache,
    /// Whether the current params use `mono` instead of the per-channel caches.
    mono_path: bool,
}

/// Picks the coefficient path for the new params and hands glide state over
/// to it.
pub fn on_patch_update(
    cutoff: &PolySignal,
    resonance: &Option<PolySignal>,
    state: &mut SvfFilterState,
    channel_state: &mut [SvfChannel],
) {
    let mono = params_mono(cutoff, resonance);
    hand_over(
        mono,
        state.mono_path,
        &mut state.mono.glides,
        channel_state.iter_mut().map(|cs| &mut cs.cache.glides),
    );
    state.mono_path = mono;
}

#[allow(clippy::too_many_arguments)]
#[inline]
pub fn run(
    response: Response,
    channels: usize,
    input: &PolySignal,
    cutoff: &PolySignal,
    resonance: &Option<PolySignal>,
    default_resonance: f32,
    sample_rate: f32,
    state: &mut SvfFilterState,
    channel_state: &mut [SvfChannel],
    out: &mut PolyOutput,
) {
    let mono = state.mono_path;
    if mono {
        state.mono.refresh(
            response,
            cutoff,
            resonance,
            default_resonance,
            0,
            sample_rate,
        );
    } else {
        for (i, cs) in channel_state.iter_mut().enumerate().take(channels) {
            cs.cache.refresh(
                response,
                cutoff,
                resonance,
                default_resonance,
                i,
                sample_rate,
            );
        }
    }

    for (i, cs) in channel_state.iter_mut().enumerate().take(channels) {
        let c = if mono {
            state.mono.coeffs
        } else {
            cs.cache.coeffs
        };
        let x = sanitize(input.get_value(i));
        out.set(i, svf_core::process(&c, &mut cs.ic1, &mut cs.ic2, x));
    }
}
