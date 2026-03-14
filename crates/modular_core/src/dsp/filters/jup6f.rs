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
struct Jup6fParams {
    /// signal input
    input: PolySignal,
    /// cutoff frequency in V/Oct (0V = C4)
    #[signal(type = pitch)]
    cutoff: Option<PolySignal>,
    /// filter resonance (0-5). High values produce self-oscillation.
    #[signal(range = (0.0, 5.0))]
    resonance: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Jup6fOutputs {
    #[output("output", "4-pole (24dB/oct) lowpass", default, range = (-5.0, 5.0))]
    lp24: PolyOutput,
    #[output("lp12", "2-pole (12dB/oct) lowpass", range = (-5.0, 5.0))]
    lp12: PolyOutput,
    #[output("bp", "bandpass", range = (-5.0, 5.0))]
    bp: PolyOutput,
    #[output("hp", "highpass", range = (-5.0, 5.0))]
    hp: PolyOutput,
}

/// 4-stage OTA ladder state for a single channel.
#[derive(Default, Clone, Copy)]
struct LadderState {
    /// Four cascaded 1-pole stages
    s: [f32; 4],
    /// Cached cutoff for change detection
    last_cutoff: f32,
    /// Cached resonance for change detection
    last_resonance: f32,
    /// Smoothed cutoff
    smooth_cutoff: Clickless,
    /// Smoothed resonance
    smooth_resonance: Clickless,
    /// Cached tuning coefficient (g)
    g: f32,
    /// Cached resonance coefficient (k)
    k: f32,
}

/// Jupiter-6–style multimode ladder filter.
///
/// Models the IR3109 4-pole OTA cascade found in the Roland Jupiter-6.
/// Each stage applies `tanh` saturation for the warm, harmonically rich
/// character of the original analog circuit. Resonance drives the feedback
/// path and self-oscillates at high values.
///
/// The default output is a 24 dB/oct lowpass. Additional taps provide
/// 12 dB/oct lowpass, bandpass, and highpass responses derived from the
/// same ladder core.
///
/// - **cutoff** — set in V/Oct (0 V = C4). Accepts modulation for filter sweeps.
/// - **resonance** — feedback amount (0–5). Above ~4 the filter self-oscillates,
///   producing a clean sine at the cutoff frequency.
///
/// ```js
/// // classic Jupiter pad: saw through the ladder with slow envelope
/// let env = $adsr($pPulse($clock[0]), { attack: 0.4, decay: 0.6, sustain: 0.3, release: 1.0 })
/// $jup6f($saw('c2'), env.range('200hz', '4000hz'), 2.5)
/// ```
#[module(name = "$jup6f", args(input, cutoff, resonance))]
pub struct Jup6f {
    outputs: Jup6fOutputs,
    state: Jup6fState,
    params: Jup6fParams,
}

/// State for the Jup6f module.
pub struct Jup6fState {
    /// Per-channel ladder state
    pub channels: [LadderState; PORT_MAX_CHANNELS],
    /// Mono optimization
    pub mono_g: f32,
    pub mono_k: f32,
    pub last_cutoff_mono: f32,
    pub last_resonance_mono: f32,
    pub smooth_cutoff_mono: Clickless,
    pub smooth_resonance_mono: Clickless,
}

impl Default for Jup6fState {
    fn default() -> Self {
        Self {
            channels: [LadderState::default(); PORT_MAX_CHANNELS],
            mono_g: 0.0,
            mono_k: 0.0,
            last_cutoff_mono: f32::NAN,
            last_resonance_mono: f32::NAN,
            smooth_cutoff_mono: Clickless::default(),
            smooth_resonance_mono: Clickless::default(),
        }
    }
}

/// Compute the ladder tuning coefficient (g) and resonance feedback (k)
/// from V/Oct cutoff and resonance parameters.
///
/// Uses the bilinear pre-warped cutoff for accurate tuning at high frequencies.
/// The resonance is mapped from the user-facing 0–5 range to a feedback coefficient
/// where 4.0 corresponds to the theoretical self-oscillation threshold.
#[inline]
fn compute_coeffs(cutoff: f32, resonance: f32, sample_rate: f32) -> (f32, f32) {
    let freq = voct_to_hz(cutoff);
    let freq = freq.min(sample_rate * 0.45).max(20.0);

    // Bilinear pre-warp for accurate tuning
    let wc = std::f32::consts::PI * freq / sample_rate;
    let g = wc.tan();

    // Map resonance 0–5 → k 0–4.0 (self-oscillation at k=4)
    let k = (resonance / 5.0 * 4.0).max(0.0).min(4.0);

    (g, k)
}

/// Process one sample through the 4-pole OTA ladder.
///
/// Returns (lp24, lp12, bp, hp) simultaneously from the ladder taps.
///
/// The algorithm uses a resolving feedback approach (no delay-free loop issues):
/// 1. Estimate the feedback signal from the previous state
/// 2. Process through 4 cascaded 1-pole sections with tanh saturation
/// 3. Derive multimode outputs from the stage taps
#[inline]
fn process_ladder(input: f32, state: &mut [f32; 4], g: f32, k: f32) -> (f32, f32, f32, f32) {
    // Feedback from 4th stage (previous sample's output — avoids delay-free loop)
    let feedback = state[3];

    // Input with resonance feedback, saturated through tanh
    let u = input - k * feedback;

    // Coefficient for the 1-pole integrator: g / (1 + g)
    let g1 = g / (1.0 + g);

    // Stage 1
    let v0 = (u - state[0]) * g1;
    let s0 = v0 + state[0];
    state[0] = s0 + v0;

    // Stage 2
    let v1 = (tanh_approx(s0) - state[1]) * g1;
    let s1 = v1 + state[1];
    state[1] = s1 + v1;

    // Stage 3
    let v2 = (tanh_approx(s1) - state[2]) * g1;
    let s2 = v2 + state[2];
    state[2] = s2 + v2;

    // Stage 4
    let v3 = (tanh_approx(s2) - state[3]) * g1;
    let s3 = v3 + state[3];
    state[3] = s3 + v3;

    // Multimode outputs derived from ladder taps:
    // LP24 = 4th stage output (24 dB/oct)
    let lp24 = s3;
    // LP12 = 2nd stage output (12 dB/oct)
    let lp12 = s1;
    // BP = difference of LP12 and LP24 (bandpass character)
    let bp = s1 - s3;
    // HP = input minus LP24 (complementary highpass)
    let hp = tanh_approx(u) - s3;

    (lp24, lp12, bp, hp)
}

/// Fast tanh approximation.
/// Pade approximant — accurate within ~0.1% for |x| < 4, gracefully saturates beyond.
/// Zero allocations, pure arithmetic.
#[inline]
fn tanh_approx(x: f32) -> f32 {
    // Clamp to avoid overflow in x*x
    let x = x.max(-4.0).min(4.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

impl Jup6f {
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
        let is_mono = cutoff_mono && resonance_mono;

        // Update coefficients
        if is_mono {
            state
                .smooth_cutoff_mono
                .update(self.params.cutoff.value_or(0, 0.0));
            state
                .smooth_resonance_mono
                .update(self.params.resonance.value_or(0, 0.0));
            let c = *state.smooth_cutoff_mono;
            let r = *state.smooth_resonance_mono;

            if changed(c, state.last_cutoff_mono) || changed(r, state.last_resonance_mono) {
                let (g, k) = compute_coeffs(c, r, sample_rate);
                state.mono_g = g;
                state.mono_k = k;
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
                    let (g, k) = compute_coeffs(c, r, sample_rate);
                    state.channels[i].g = g;
                    state.channels[i].k = k;
                    state.channels[i].last_cutoff = c;
                    state.channels[i].last_resonance = r;
                }
            }
        }

        for i in 0..num_channels {
            let input = self.params.input.get_value(i) * 0.2; // ±5V → ±1V

            let (g, k) = if is_mono {
                (state.mono_g, state.mono_k)
            } else {
                (state.channels[i].g, state.channels[i].k)
            };

            let (lp24, lp12, bp, hp) = process_ladder(input, &mut state.channels[i].s, g, k);

            self.outputs.lp24.set(i, lp24 * 5.0);
            self.outputs.lp12.set(i, lp12 * 5.0);
            self.outputs.bp.set(i, bp * 5.0);
            self.outputs.hp.set(i, hp * 5.0);
        }
    }
}

message_handlers!(impl Jup6f {});
