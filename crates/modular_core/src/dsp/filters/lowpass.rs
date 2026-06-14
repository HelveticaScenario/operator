use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::{changed, sanitize, voct_to_hz},
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct LowpassFilterParams {
    /// signal input
    input: PolySignal,
    /// cutoff frequency in V/Oct (0V = C4)
    #[signal(type = pitch, default = 0.0, range = (-5.0, 5.0))]
    cutoff: PolySignal,
    /// filter resonance (0-5)
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    resonance: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LowpassFilterOutputs {
    #[output("output", "filtered signal", default)]
    sample: PolyOutput,
}

/// Lowpass filter that attenuates frequencies above the cutoff point.
///
/// Use it to tame bright timbres, create bass-heavy sounds, or build classic
/// subtractive synth patches. Sweeping the cutoff with an envelope or LFO
/// produces the familiar filter-sweep effect.
///
/// - **cutoff** — set in V/Oct (0 V = C4). Accepts modulation for filter sweeps.
/// - **resonance** — boosts frequencies near the cutoff (0–5). High values
///   produce a ringing peak; very high values cause self-oscillation.
///
/// ```js
/// // subtractive bass: saw through a lowpass with envelope on cutoff
/// let env = $adsr($pPulse($clock[0]), { attack: 0.01, decay: 0.3, sustain: 1, release: 0.4 })
/// $lpf($saw('c2'), env.range('200hz', '2000hz'))
/// ```
#[module(name = "$lpf", args(input, cutoff, resonance))]
pub struct LowpassFilter {
    outputs: LowpassFilterOutputs,
    state: LowpassFilterState,
    channel_state: Box<[LowpassChannel]>,
    params: LowpassFilterParams,
}

/// Per-channel filter state.
#[derive(Clone, Copy)]
struct LowpassChannel {
    /// Audio-rate biquad state.
    z1: f32,
    z2: f32,
    /// Cached coefficients (control-rate).
    coeffs: BiquadCoeffs,
    /// Last seen params (for change detection); `NaN` = never computed.
    last_cutoff: f32,
    last_resonance: f32,
    /// Parameter smoothing to prevent clicks on sudden changes.
    smooth_cutoff: Clickless,
    smooth_resonance: Clickless,
}

impl Default for LowpassChannel {
    fn default() -> Self {
        Self {
            z1: 0.0,
            z2: 0.0,
            coeffs: BiquadCoeffs::default(),
            last_cutoff: f32::NAN,
            last_resonance: f32::NAN,
            smooth_cutoff: Clickless::default(),
            smooth_resonance: Clickless::default(),
        }
    }
}

/// Module-level state for the LowpassFilter (mono-input fast path).
struct LowpassFilterState {
    coeffs_mono: BiquadCoeffs,
    last_cutoff_mono: f32,
    last_resonance_mono: f32,
    smooth_cutoff_mono: Clickless,
    smooth_resonance_mono: Clickless,
}

impl Default for LowpassFilterState {
    fn default() -> Self {
        Self {
            coeffs_mono: BiquadCoeffs::default(),
            last_cutoff_mono: f32::NAN,
            last_resonance_mono: f32::NAN,
            smooth_cutoff_mono: Clickless::default(),
            smooth_resonance_mono: Clickless::default(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

fn compute_biquad(cutoff: f32, resonance: f32, sample_rate: f32) -> BiquadCoeffs {
    let freq = voct_to_hz(cutoff);
    let freq = freq.min(sample_rate * 0.45).max(20.0);

    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin = omega.sin();
    let cos = omega.cos();
    let q = (resonance / 5.0 * 9.0 + 0.5).max(0.5);
    let alpha = sin / (2.0 * q);

    let b0 = (1.0 - cos) / 2.0;
    let b1 = 1.0 - cos;
    let b2 = (1.0 - cos) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos;
    let a2 = 1.0 - alpha;

    BiquadCoeffs {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

impl LowpassFilter {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();

        let cutoff_mono = self.params.cutoff.is_monophonic();
        let resonance_mono = self
            .params
            .resonance
            .as_ref()
            .is_some_and(|s| s.is_monophonic());

        // Update coefficients with smoothed params to prevent clicks
        if cutoff_mono && resonance_mono {
            let state = &mut self.state;
            state
                .smooth_cutoff_mono
                .update(self.params.cutoff.get_value(0));
            state
                .smooth_resonance_mono
                .update(self.params.resonance.value_or(0, 0.0));
            let c = *state.smooth_cutoff_mono;
            let r = *state.smooth_resonance_mono;

            if changed(c, state.last_cutoff_mono) || changed(r, state.last_resonance_mono) {
                state.coeffs_mono = compute_biquad(c, r, sample_rate);
                state.last_cutoff_mono = c;
                state.last_resonance_mono = r;
            }
        } else {
            for i in 0..channels as usize {
                let cs = &mut self.channel_state[i];
                cs.smooth_cutoff.update(self.params.cutoff.get_value(i));
                cs.smooth_resonance
                    .update(self.params.resonance.value_or(i, 0.0));
                let c = *cs.smooth_cutoff;
                let r = *cs.smooth_resonance;

                if changed(c, cs.last_cutoff) || changed(r, cs.last_resonance) {
                    cs.coeffs = compute_biquad(c, r, sample_rate);
                    cs.last_cutoff = c;
                    cs.last_resonance = r;
                }
            }
        }

        for i in 0..channels as usize {
            let input = self.params.input.get_value(i);

            let c = if cutoff_mono && resonance_mono {
                self.state.coeffs_mono
            } else {
                self.channel_state[i].coeffs
            };

            let cs = &mut self.channel_state[i];
            let w = sanitize(input - c.a1 * cs.z1 - c.a2 * cs.z2);
            let y = c.b0 * w + c.b1 * cs.z1 + c.b2 * cs.z2;

            cs.z2 = cs.z1;
            cs.z1 = w;
            self.outputs.sample.set(i, y);
        }
    }
}

message_handlers!(impl LowpassFilter {});
