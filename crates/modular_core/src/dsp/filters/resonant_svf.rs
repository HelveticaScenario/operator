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

fn compute_coeffs(response: Response, cutoff: f32, resonance: f32, sample_rate: f32) -> SvfCoeffs {
    let freq = voct_to_hz(cutoff).min(sample_rate * 0.45).max(20.0);
    let g = (std::f32::consts::PI * freq / sample_rate).tan();
    let k = 1.0 / (resonance / 5.0 * 9.0 + 0.5).max(0.5);
    match response {
        Response::Lowpass => SvfCoeffs::new(g, k, 0.0, 0.0, 1.0),
        Response::Highpass => SvfCoeffs::new(g, k, 1.0, -k, -1.0),
        Response::Bandpass => SvfCoeffs::new(g, k, 0.0, k, 0.0),
    }
}

/// Glided params and the coefficients derived from them. `NaN` = never computed.
#[derive(Clone, Copy)]
pub struct CoeffCache {
    coeffs: SvfCoeffs,
    last_cutoff: f32,
    last_resonance: f32,
    cutoff: SwapGlide,
    resonance: SwapGlide,
}

impl Default for CoeffCache {
    fn default() -> Self {
        Self {
            coeffs: SvfCoeffs::default(),
            last_cutoff: f32::NAN,
            last_resonance: f32::NAN,
            cutoff: SwapGlide::default(),
            resonance: SwapGlide::default(),
        }
    }
}

impl CoeffCache {
    fn arm(&mut self) {
        self.cutoff.arm();
        self.resonance.arm();
    }

    #[inline]
    fn refresh(&mut self, response: Response, cutoff: f32, resonance: f32, sample_rate: f32) {
        self.cutoff.update(cutoff, sample_rate);
        self.resonance.update(resonance, sample_rate);
        let (c, r) = (*self.cutoff, *self.resonance);
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
    was_mono: bool,
}

fn params_mono(cutoff: &PolySignal, resonance: &Option<PolySignal>) -> bool {
    cutoff.is_monophonic() && resonance.as_ref().is_none_or(|s| s.is_monophonic())
}

/// Arms every glide that the coming samples will read. When an edit switches
/// between the shared and per-channel coefficient paths, the newly active
/// path inherits the other's glide state, so it continues from what was
/// actually playing.
pub fn on_patch_update(
    cutoff: &PolySignal,
    resonance: &Option<PolySignal>,
    state: &mut SvfFilterState,
    channel_state: &mut [SvfChannel],
) {
    let mono = params_mono(cutoff, resonance);
    if mono {
        if !state.was_mono {
            if let Some(first) = channel_state.first() {
                state.mono = first.cache;
            }
        }
        state.mono.arm();
    } else {
        for cs in channel_state.iter_mut() {
            if state.was_mono {
                cs.cache = state.mono;
            }
            cs.cache.arm();
        }
    }
    state.was_mono = mono;
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
    let mono = params_mono(cutoff, resonance);
    if mono {
        state.mono.refresh(
            response,
            cutoff.get_value(0),
            resonance.value_or(0, default_resonance),
            sample_rate,
        );
    } else {
        for (i, cs) in channel_state.iter_mut().enumerate().take(channels) {
            cs.cache.refresh(
                response,
                cutoff.get_value(i),
                resonance.value_or(i, default_resonance),
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
