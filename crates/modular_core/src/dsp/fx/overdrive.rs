//! Multi-mode overdrive with pre-emphasis tone shaping and 2× oversampling.
//!
//! Modes:
//!   - `Soft` — tanh saturation
//!   - `Hard` — hard clipping
//!   - `Asym` — asymmetric tube-style saturation (even harmonic content)

use deserr::Deserr;
use schemars::JsonSchema;
use std::f32::consts::PI;

use crate::dsp::utils::halfband::{Halfband2xDown, Halfband2xUp};
use crate::dsp::utils::one_pole::OnePole;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt};
use crate::types::Clickless;

/// Saturation algorithm.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "snake_case")]
#[deserr(rename_all = lowercase)]
pub enum OverdriveMode {
    /// Smooth tanh-style saturation.
    #[default]
    Soft,
    /// Hard clipping at ±1.
    Hard,
    /// Asymmetric tube-style saturation with even harmonics.
    Asym,
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct OverdriveParams {
    /// input signal to overdrive (bipolar, typically -5 to 5)
    input: PolySignal,
    /// drive amount (0 = unity gain, 5 = ~30 dB pre-shaper gain)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    drive: PolySignal,
    /// tone (-5 = dark, 0 = neutral, +5 = bright). Pre-emphasises highs into
    /// the shaper at positive values and de-emphasises them after, and vice versa.
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// saturation mode (defaults to soft)
    #[serde(default)]
    #[deserr(default)]
    mode: Option<OverdriveMode>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct OverdriveOutputs {
    #[output("output", "overdriven signal output", default)]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    drive: Clickless,
    tone: Clickless,
    up: Halfband2xUp,
    down: Halfband2xDown,
    tilt_pre: OnePole,
    tilt_post: OnePole,
    /// DC blocker (one-pole high-pass) — `y[n] = x[n] - x[n-1] + R · y[n-1]`.
    dc_prev_in: f32,
    dc_prev_out: f32,
}

#[derive(Default)]
struct OverdriveState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
    tilt_coeff: f32,
    dc_block_coeff: f32,
}

/// Tilt-EQ pivot frequency, in Hz.
const TILT_PIVOT_HZ: f32 = 1500.0;

/// DC-blocker corner frequency, in Hz.
const DC_BLOCK_FC_HZ: f32 = 20.0;

/// Maximum drive multiplier (gain at drive = 5).
const MAX_DRIVE_GAIN: f32 = 32.0;

/// Pre/post tilt-EQ high-band gain at tone = ±5. Maps full ±5 range to a
/// 3:1 boost/cut ratio. Larger values = more aggressive tone shift.
const TONE_RANGE: f32 = 3.0;

/// Multi-mode overdrive saturator. Drive boosts the input into a soft, hard,
/// or asymmetric shaper, with a tone control applying pre-emphasis before
/// distortion and matching de-emphasis after. Internally oversampled 2× to
/// reduce aliasing.
#[module(name = "$overdrive", args(input, drive), has_init)]
pub struct Overdrive {
    outputs: OverdriveOutputs,
    state: OverdriveState,
    params: OverdriveParams,
}

impl Overdrive {
    /// Compute sample-rate-dependent coefficients once at construction.
    /// Invoked by the `#[module]` proc macro on the main thread.
    fn init(&mut self, sample_rate: f32) {
        // Tilt-EQ runs inside the 2× oversampled inner loop.
        let upper_rate = (sample_rate * 2.0).max(1.0);
        let tilt = 1.0 - (-2.0 * PI * TILT_PIVOT_HZ / upper_rate).exp();
        self.state.tilt_coeff = tilt.clamp(0.0, 1.0);

        // DC blocker runs at the base rate before the upsampler.
        let base_rate = sample_rate.max(1.0);
        self.state.dc_block_coeff =
            (1.0 - (2.0 * PI * DC_BLOCK_FC_HZ / base_rate)).clamp(0.0, 1.0);
    }

    fn update(&mut self, _sample_rate: f32) {
        let tilt_coeff = self.state.tilt_coeff;
        let dc_coeff = self.state.dc_block_coeff;
        let mode = self.params.mode.unwrap_or_default();
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let input = self.params.input.get_value(ch);
            let drive_raw = self.params.drive.get_value(ch);
            let tone_raw = self.params.tone.value_or(ch, 0.0);

            state.drive.update(drive_raw);
            state.tone.update(tone_raw);
            let drive = (*state.drive).clamp(0.0, 5.0);
            let tone = (*state.tone).clamp(-5.0, 5.0);

            let g = 1.0 + drive * ((MAX_DRIVE_GAIN - 1.0) / 5.0);
            // tone in [-5, 5] maps to symmetric high-band gain pair via 3^(tone/5):
            // tone=-5 → pre=1/3, post=3; tone=0 → 1, 1; tone=+5 → pre=3, post=1/3.
            // Linear-signal cascade (pre · post) = 1 at all settings — true
            // pre-emphasis / de-emphasis pair, no dead zone.
            let amount = tone * 0.2;
            let pre_high_gain = TONE_RANGE.powf(amount);
            let post_high_gain = TONE_RANGE.powf(-amount);

            state.tilt_pre.set_coeff(tilt_coeff);
            state.tilt_post.set_coeff(tilt_coeff);

            // DC-block the input at base rate before upsampling.
            let x_norm = input / 5.0;
            let dc_out = x_norm - state.dc_prev_in + dc_coeff * state.dc_prev_out;
            state.dc_prev_in = x_norm;
            state.dc_prev_out = dc_out;

            let (e, o) = state.up.process(dc_out);
            let e_shaped = process_one(
                e,
                g,
                pre_high_gain,
                post_high_gain,
                mode,
                &mut state.tilt_pre,
                &mut state.tilt_post,
            );
            let o_shaped = process_one(
                o,
                g,
                pre_high_gain,
                post_high_gain,
                mode,
                &mut state.tilt_pre,
                &mut state.tilt_post,
            );
            let y = state.down.process(e_shaped, o_shaped);
            self.outputs.sample.set(ch, y * 5.0);
        }
    }
}

#[inline]
fn process_one(
    x: f32,
    g: f32,
    pre_high_gain: f32,
    post_high_gain: f32,
    mode: OverdriveMode,
    tilt_pre: &mut OnePole,
    tilt_post: &mut OnePole,
) -> f32 {
    let lp_in = tilt_pre.process(x);
    let hp_in = x - lp_in;
    let pre = lp_in + hp_in * pre_high_gain;

    let driven = pre * g;
    let shaped = match mode {
        OverdriveMode::Soft => driven.tanh(),
        OverdriveMode::Hard => driven.clamp(-1.0, 1.0),
        OverdriveMode::Asym => {
            if driven >= 0.0 {
                driven.tanh()
            } else {
                (driven * 0.7).tanh() * 0.85
            }
        }
    };

    let lp_out = tilt_post.process(shaped);
    let hp_out = shaped - lp_out;
    lp_out + hp_out * post_high_gain
}

message_handlers!(impl Overdrive {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};

    fn make(params: OverdriveParams) -> Overdrive {
        let mut outputs = OverdriveOutputs::default();
        outputs.set_all_channels(1);
        let mut od = Overdrive {
            params,
            outputs,
            _channel_count: 1,
            state: OverdriveState::default(),
        };
        od.init(48000.0);
        od
    }

    fn default_params(input: f32, drive: f32, mode: OverdriveMode) -> OverdriveParams {
        OverdriveParams {
            input: PolySignal::mono(Signal::Volts(input)),
            drive: PolySignal::mono(Signal::Volts(drive)),
            tone: None,
            mode: Some(mode),
        }
    }

    #[test]
    fn output_is_finite() {
        let mut od = make(default_params(2.0, 3.0, OverdriveMode::Soft));
        for _ in 0..200 {
            od.update(48000.0);
            let y = od.outputs.sample.get(0);
            assert!(y.is_finite(), "output should be finite, got {y}");
        }
    }

    #[test]
    fn output_bounded_for_soft_clip() {
        let mut od = make(default_params(100.0, 5.0, OverdriveMode::Soft));
        // Warm up past the halfband transient on a hard step input.
        for _ in 0..2000 {
            od.update(48000.0);
        }
        for _ in 0..1000 {
            od.update(48000.0);
            let y = od.outputs.sample.get(0);
            assert!(y.abs() <= 5.05, "soft-clip output should be bounded, got {y}");
        }
    }

    #[test]
    fn output_bounded_for_hard_clip() {
        let mut od = make(default_params(100.0, 5.0, OverdriveMode::Hard));
        for _ in 0..2000 {
            od.update(48000.0);
        }
        for _ in 0..1000 {
            od.update(48000.0);
            let y = od.outputs.sample.get(0);
            assert!(y.abs() <= 5.05, "hard-clip output should be bounded, got {y}");
        }
    }

    #[test]
    fn dc_input_blocked_to_silence() {
        // 1 V DC into the saturator should be removed by the DC blocker; output
        // converges to ~0 regardless of drive.
        let mut od = make(default_params(1.0, 0.0, OverdriveMode::Soft));
        let mut last = 0.0;
        for _ in 0..4000 {
            od.update(48000.0);
            last = od.outputs.sample.get(0);
        }
        assert!(
            last.abs() < 0.05,
            "expected near 0 V after DC blocker, got {last}"
        );
    }

    #[test]
    fn high_drive_increases_ac_output_magnitude() {
        // Same AC input through different drives — higher drive should yield
        // a larger peak, even with the DC blocker in place.
        let mut low = make(default_params(0.0, 0.0, OverdriveMode::Soft));
        let mut high = make(default_params(0.0, 5.0, OverdriveMode::Soft));
        let f_norm = 0.01_f32; // ~480 Hz @ 48 kHz — well above DC corner.
        let mut low_peak = 0.0_f32;
        let mut high_peak = 0.0_f32;
        for n in 0..4000 {
            let x = (2.0 * PI * f_norm * n as f32).sin();
            low.params.input = PolySignal::mono(Signal::Volts(x));
            high.params.input = PolySignal::mono(Signal::Volts(x));
            low.update(48000.0);
            high.update(48000.0);
            if n >= 1000 {
                low_peak = low_peak.max(low.outputs.sample.get(0).abs());
                high_peak = high_peak.max(high.outputs.sample.get(0).abs());
            }
        }
        assert!(
            high_peak > low_peak,
            "high drive should produce larger peak, got low={low_peak}, high={high_peak}"
        );
    }
}
