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
struct BandpassFilterParams {
    /// signal input
    input: PolySignal,
    /// center frequency in V/Oct (0V = C4)
    #[signal(type = pitch)]
    center: PolySignal,
    /// filter resonance — controls bandwidth (0–5)
    #[signal(default = 1.0, range = (0.0, 5.0))]
    #[deserr(default)]
    resonance: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BandpassFilterOutputs {
    #[output("output", "filtered signal", default)]
    sample: PolyOutput,
}

#[derive(Clone, Copy, Default)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Default, Clone, Copy)]
struct BpfChannelState {
    z1: f32,
    z2: f32,
    coeffs: BiquadCoeffs,
    last_center: f32,
    last_q: f32,
    smooth_center: Clickless,
    smooth_resonance: Clickless,
}

fn compute_bpf_biquad(center: f32, resonance: f32, sample_rate: f32) -> BiquadCoeffs {
    let freq = voct_to_hz(center);
    let freq = freq.min(sample_rate * 0.45).max(20.0);

    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let q = (resonance / 5.0 * 9.0 + 0.5).max(0.5);
    let alpha = sin_omega / (2.0 * q);

    let b0 = alpha;
    let b1 = 0.0;
    let b2 = -alpha;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_omega;
    let a2 = 1.0 - alpha;

    BiquadCoeffs {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

/// Bandpass filter that passes frequencies near the center frequency and
/// attenuates everything else.
///
/// Use it to isolate a frequency region, create vowel-like tones, or
/// build resonant "wah" effects by sweeping the center frequency.
///
/// - **center** — center frequency in V/Oct (0 V = C4).
/// - **resonance** — controls bandwidth (0–5). Higher values narrow the
///   passband for a more pronounced, ringing sound.
///
/// ```js
/// // resonant bandpass sweep on noise
/// $bpf($noise("white"), $sine('0.5hz').range('440hz', '1200hz'), 3)
/// ```
#[module(name = "$bpf", args(input, center, resonance))]
pub struct BandpassFilter {
    outputs: BandpassFilterOutputs,
    state: BandpassFilterState,
    channel_state: Box<[BpfChannelState]>,
    params: BandpassFilterParams,
}

/// State for the BandpassFilter module.
struct BandpassFilterState {
    /// Mono optimization
    coeffs_mono: BiquadCoeffs,
    last_center_mono: f32,
    last_q_mono: f32,
    smooth_center_mono: Clickless,
    smooth_resonance_mono: Clickless,
}

impl Default for BandpassFilterState {
    fn default() -> Self {
        Self {
            coeffs_mono: BiquadCoeffs::default(),
            last_center_mono: f32::NAN,
            last_q_mono: f32::NAN,
            smooth_center_mono: Clickless::default(),
            smooth_resonance_mono: Clickless::default(),
        }
    }
}

impl BandpassFilter {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        let center_mono = self.params.center.is_monophonic();
        let resonance_mono = self
            .params
            .resonance
            .as_ref()
            .is_some_and(|s| s.is_monophonic());

        // Update coefficients with smoothed params to prevent clicks
        if center_mono && resonance_mono {
            self.state
                .smooth_center_mono
                .update(self.params.center.get_value(0));
            self.state
                .smooth_resonance_mono
                .update(self.params.resonance.value_or(0, 1.0));
            let c = *self.state.smooth_center_mono;
            let r = *self.state.smooth_resonance_mono;

            if changed(c, self.state.last_center_mono) || changed(r, self.state.last_q_mono) {
                self.state.coeffs_mono = compute_bpf_biquad(c, r, sample_rate);
                self.state.last_center_mono = c;
                self.state.last_q_mono = r;
            }
        } else {
            for i in 0..num_channels {
                self.channel_state[i]
                    .smooth_center
                    .update(self.params.center.get_value(i));
                self.channel_state[i]
                    .smooth_resonance
                    .update(self.params.resonance.value_or(i, 1.0));
                let c = *self.channel_state[i].smooth_center;
                let r = *self.channel_state[i].smooth_resonance;

                if changed(c, self.channel_state[i].last_center)
                    || changed(r, self.channel_state[i].last_q)
                {
                    self.channel_state[i].coeffs = compute_bpf_biquad(c, r, sample_rate);
                    self.channel_state[i].last_center = c;
                    self.channel_state[i].last_q = r;
                }
            }
        }

        for i in 0..num_channels {
            let input = self.params.input.get_value(i);

            let c = if center_mono && resonance_mono {
                self.state.coeffs_mono
            } else {
                self.channel_state[i].coeffs
            };

            let ch_state = &mut self.channel_state[i];
            let w = input - c.a1 * ch_state.z1 - c.a2 * ch_state.z2;
            let w = sanitize(w);
            let y = c.b0 * w + c.b1 * ch_state.z1 + c.b2 * ch_state.z2;

            ch_state.z2 = ch_state.z1;
            ch_state.z1 = w;
            self.outputs.sample.set(i, y);
        }
    }
}

message_handlers!(impl BandpassFilter {});
