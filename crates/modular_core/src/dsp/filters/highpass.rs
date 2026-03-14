use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    dsp::utils::{changed, voct_to_hz},
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
    PORT_MAX_CHANNELS,
};

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct HighpassFilterParams {
    /// signal input
    input: PolySignal,
    /// cutoff frequency in V/Oct (0V = C4)
    #[signal(type = pitch)]
    cutoff: Option<PolySignal>,
    /// filter resonance (0-5)
    #[signal(range = (0.0, 5.0))]
    resonance: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct HighpassFilterOutputs {
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
struct HpfChannelState {
    z1: f32,
    z2: f32,
    coeffs: BiquadCoeffs,
    last_cutoff: f32,
    last_resonance: f32,
    smooth_cutoff: Clickless,
    smooth_resonance: Clickless,
}

fn compute_hpf_biquad(cutoff: f32, resonance: f32, sample_rate: f32) -> BiquadCoeffs {
    let freq = voct_to_hz(cutoff);
    let freq = freq.min(sample_rate * 0.45).max(20.0);

    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let q = (resonance / 5.0 * 9.0 + 0.5).max(0.5);
    let alpha = sin_omega / (2.0 * q);

    let b0 = (1.0 + cos_omega) / 2.0;
    let b1 = -(1.0 + cos_omega);
    let b2 = (1.0 + cos_omega) / 2.0;
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

/// Highpass filter that attenuates frequencies below the cutoff point.
///
/// Use it to remove low-end rumble, thin out a sound, or create rising
/// filter effects. Pairs well with lowpass filters for isolating a
/// frequency band.
///
/// - **cutoff** — set in V/Oct (0 V = C4). Accepts modulation for filter sweeps.
/// - **resonance** — boosts frequencies near the cutoff (0–5). High values
///   produce a ringing peak.
///
/// ```js
/// // remove low end from a noise source
/// $hpf($noise("white"), 'a3', 1)
/// ```
#[module(name = "$hpf", args(input, cutoff, resonance))]
pub struct HighpassFilter {
    outputs: HighpassFilterOutputs,
    state: HighpassFilterState,
    params: HighpassFilterParams,
}

/// State for the HighpassFilter module.
pub struct HighpassFilterState {
    /// Per-channel state
    pub channels: [HpfChannelState; PORT_MAX_CHANNELS],
    /// Mono optimization
    pub coeffs_mono: BiquadCoeffs,
    pub last_cutoff_mono: f32,
    pub last_resonance_mono: f32,
    pub smooth_cutoff_mono: Clickless,
    pub smooth_resonance_mono: Clickless,
}

impl Default for HighpassFilterState {
    fn default() -> Self {
        Self {
            channels: [HpfChannelState::default(); PORT_MAX_CHANNELS],
            coeffs_mono: BiquadCoeffs::default(),
            last_cutoff_mono: f32::NAN,
            last_resonance_mono: f32::NAN,
            smooth_cutoff_mono: Clickless::default(),
            smooth_resonance_mono: Clickless::default(),
        }
    }
}

impl HighpassFilter {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let state = &mut self.state;

        let cutoff_mono = self
            .params
            .cutoff
            .as_ref()
            .is_some_and(|s| s.is_monophonic());
        let resonance_mono = self
            .params
            .resonance
            .as_ref()
            .is_some_and(|s| s.is_monophonic());

        // Update coefficients with smoothed params to prevent clicks
        if cutoff_mono && resonance_mono {
            state
                .smooth_cutoff_mono
                .update(self.params.cutoff.value_or(0, 0.0));
            state
                .smooth_resonance_mono
                .update(self.params.resonance.value_or(0, 0.0));
            let c = *state.smooth_cutoff_mono;
            let r = *state.smooth_resonance_mono;

            if changed(c, state.last_cutoff_mono) || changed(r, state.last_resonance_mono) {
                state.coeffs_mono = compute_hpf_biquad(c, r, sample_rate);
                state.last_cutoff_mono = c;
                state.last_resonance_mono = r;
            }
        } else {
            for i in 0..num_channels {
                state.channels[i]
                    .smooth_cutoff
                    .update(self.params.cutoff.value_or(i, 0.0));
                state.channels[i]
                    .smooth_resonance
                    .update(self.params.resonance.value_or(i, 0.0));
                let c = *state.channels[i].smooth_cutoff;
                let r = *state.channels[i].smooth_resonance;

                if changed(c, state.channels[i].last_cutoff)
                    || changed(r, state.channels[i].last_resonance)
                {
                    state.channels[i].coeffs = compute_hpf_biquad(c, r, sample_rate);
                    state.channels[i].last_cutoff = c;
                    state.channels[i].last_resonance = r;
                }
            }
        }

        for i in 0..num_channels {
            let input = self.params.input.get_value(i);

            let c = if cutoff_mono && resonance_mono {
                state.coeffs_mono
            } else {
                state.channels[i].coeffs
            };

            let ch_state = &mut state.channels[i];
            let w = input - c.a1 * ch_state.z1 - c.a2 * ch_state.z2;
            let y = c.b0 * w + c.b1 * ch_state.z1 + c.b2 * ch_state.z2;

            ch_state.z2 = ch_state.z1;
            ch_state.z1 = w;
            self.outputs.sample.set(i, y);
        }
    }
}

message_handlers!(impl HighpassFilter {});
