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
struct LowpassFilterParams {
    /// signal input
    input: PolySignal,
    /// cutoff frequency in V/Oct (0V = C4)
    #[signal(type = pitch, default = 0.0, range = (-5.0, 5.0))]
    cutoff: Option<PolySignal>,
    /// filter resonance (0-5)
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
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
    params: LowpassFilterParams,
}

/// State for the LowpassFilter module.
pub struct LowpassFilterState {
    /// Per-channel state (audio-rate)
    pub z1: [f32; PORT_MAX_CHANNELS],
    pub z2: [f32; PORT_MAX_CHANNELS],
    /// Cached coefficients (control-rate)
    pub coeffs: [BiquadCoeffs; PORT_MAX_CHANNELS],
    /// Last seen params (for change detection)
    pub last_cutoff: [f32; PORT_MAX_CHANNELS],
    pub last_resonance: [f32; PORT_MAX_CHANNELS],
    /// Parameter smoothing to prevent clicks on sudden changes
    pub smooth_cutoff: [Clickless; PORT_MAX_CHANNELS],
    pub smooth_resonance: [Clickless; PORT_MAX_CHANNELS],
    /// Mono optimization
    pub coeffs_mono: BiquadCoeffs,
    pub last_cutoff_mono: f32,
    pub last_resonance_mono: f32,
    pub smooth_cutoff_mono: Clickless,
    pub smooth_resonance_mono: Clickless,
}

impl Default for LowpassFilterState {
    fn default() -> Self {
        Self {
            z1: [0.0; PORT_MAX_CHANNELS],
            z2: [0.0; PORT_MAX_CHANNELS],
            coeffs: [BiquadCoeffs::default(); PORT_MAX_CHANNELS],
            last_cutoff: [f32::NAN; PORT_MAX_CHANNELS],
            last_resonance: [f32::NAN; PORT_MAX_CHANNELS],
            smooth_cutoff: [Clickless::default(); PORT_MAX_CHANNELS],
            smooth_resonance: [Clickless::default(); PORT_MAX_CHANNELS],
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
                state.coeffs_mono = compute_biquad(c, r, sample_rate);
                state.last_cutoff_mono = c;
                state.last_resonance_mono = r;
            }
        } else {
            for i in 0..channels as usize {
                state.smooth_cutoff[i].update(self.params.cutoff.value_or(i, 0.0));
                state.smooth_resonance[i].update(self.params.resonance.value_or(i, 0.0));
                let c = *state.smooth_cutoff[i];
                let r = *state.smooth_resonance[i];

                if changed(c, state.last_cutoff[i]) || changed(r, state.last_resonance[i]) {
                    state.coeffs[i] = compute_biquad(c, r, sample_rate);
                    state.last_cutoff[i] = c;
                    state.last_resonance[i] = r;
                }
            }
        }

        for i in 0..channels as usize {
            let input = self.params.input.get_value(i);

            let c = if cutoff_mono && resonance_mono {
                state.coeffs_mono
            } else {
                state.coeffs[i]
            };

            let w = input - c.a1 * state.z1[i] - c.a2 * state.z2[i];
            let y = c.b0 * w + c.b1 * state.z1[i] + c.b2 * state.z2[i];

            state.z2[i] = state.z1[i];
            state.z1[i] = w;
            self.outputs.sample.set(i, y);
        }
    }
}

message_handlers!(impl LowpassFilter {});
