//! Dattorro reverb module.
//!
//! Implements Jon Dattorro's plate reverberator algorithm from
//! "Effect Design Part 1: Reverberator and Other Filters" (JAES, 1997).

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::delay_line::DelayLine;
use crate::dsp::utils::map_range;
use crate::poly::{MonoSignal, MonoSignalExt, PolyOutput, PolySignal};
use crate::types::Clickless;

// ─── Dattorro delay lengths (reference sample rate: 29761 Hz) ────────────────

const REF_SAMPLE_RATE: f32 = 29761.0;

// Input diffuser allpass delay lengths
const INPUT_DIFF_1: f32 = 142.0;
const INPUT_DIFF_2: f32 = 107.0;
const INPUT_DIFF_3: f32 = 379.0;
const INPUT_DIFF_4: f32 = 277.0;

// Tank decay diffusion allpass delay lengths
const DECAY_DIFF_1: f32 = 672.0;
const DECAY_DIFF_2: f32 = 908.0;

// Tank delay line lengths
const TANK_DELAY_1: f32 = 4453.0;
const TANK_DELAY_2: f32 = 4217.0;

// Output tap positions (from Dattorro's Table 1)
// Left output taps
const TAP_L1: f32 = 266.0;
const TAP_L2: f32 = 2974.0;
const TAP_L3: f32 = 1913.0;
const TAP_L4: f32 = 1996.0;
const TAP_L5: f32 = 1990.0;
const TAP_L6: f32 = 187.0;
const TAP_L7: f32 = 1066.0;

// Right output taps
const TAP_R1: f32 = 353.0;
const TAP_R2: f32 = 3627.0;
const TAP_R3: f32 = 1228.0;
const TAP_R4: f32 = 2673.0;
const TAP_R5: f32 = 2111.0;
const TAP_R6: f32 = 335.0;
const TAP_R7: f32 = 121.0;

// Maximum predelay in seconds
const MAX_PREDELAY_SECS: f32 = 0.5;

// Modulation excursion depth at the reference sample rate (in samples).
// From Dattorro's paper: the internal LFO sweeps the decay diffusion
// allpass delay lengths by ±16 samples at 29761 Hz (~0.54ms).
const REF_MOD_EXCURSION: f32 = 16.0;

/// Scale a reference delay length to the actual sample rate, then multiply
/// by the size factor. Returns an integer sample count.
#[inline]
fn scale_delay(ref_samples: f32, sample_rate: f32, size: f32) -> usize {
    ((ref_samples * sample_rate / REF_SAMPLE_RATE) * size).round() as usize
}

/// Like [`scale_delay`] but returns a fractional sample count for use with
/// `read_linear` / `allpass_linear`, avoiding zipper noise when `size`
/// changes continuously.
#[inline]
fn scale_delay_f(ref_samples: f32, sample_rate: f32, size: f32) -> f32 {
    (ref_samples * sample_rate / REF_SAMPLE_RATE) * size
}

// ─── Params ──────────────────────────────────────────────────────────────────

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct DattorroParams {
    /// audio input (even channels → left, odd channels → right)
    input: PolySignal,
    /// reverb decay time (-5 to 5, default 0)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    decay: Option<MonoSignal>,
    /// high-frequency damping in the reverb tank (-5 to 5, default 0)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    damping: Option<MonoSignal>,
    /// room size — scales all delay line lengths (-5 to 5, default 0)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    size: Option<MonoSignal>,
    /// predelay time in seconds (0 to 0.5, default 0)
    #[signal(default = 0.0, range = (0.0, 0.5))]
    #[deserr(default)]
    predelay: Option<MonoSignal>,
    /// input diffusion amount (0 to 5, default 3.5)
    #[signal(default = 3.5, range = (0.0, 5.0))]
    #[deserr(default)]
    diffusion: Option<MonoSignal>,
    /// external tank modulation signal (-5 to 5, default 0, not clamped)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    modulation: Option<MonoSignal>,
}

// ─── Outputs ─────────────────────────────────────────────────────────────────

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DattorroOutputs {
    #[output("output", "stereo reverb output (ch0=left, ch1=right)", default)]
    sample: PolyOutput,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Pre-allocated state for the Dattorro reverb tank.
///
/// All `DelayLine`s default to empty and are allocated in `init()`.
#[derive(Default)]
struct DattorroState {
    // Predelay
    predelay_l: DelayLine,
    predelay_r: DelayLine,

    // Input diffusers (4 cascaded allpass filters)
    input_diff: [DelayLine; 4],

    // Tank: left path
    decay_diff_l: DelayLine,
    tank_delay_l: DelayLine,
    damp_state_l: f32,

    // Tank: right path
    decay_diff_r: DelayLine,
    tank_delay_r: DelayLine,
    damp_state_r: f32,

    // Cross-feedback
    feedback_l: f32,
    feedback_r: f32,

    // Parameter smoothing — prevents zipper noise when params change
    // continuously, since these control delay lengths where
    // discontinuities get amplified by the feedback loop.
    smoothed_size: Clickless,
    smoothed_predelay: Clickless,
    // Decay diffusion allpass gains are inside the feedback loop, so
    // abrupt changes create transients that recirculate.
    smoothed_decay_diff_1: Clickless,
    smoothed_decay_diff_2: Clickless,

    // Output tap positions, scaled by sample rate only (NOT by size).
    // Taps are fixed positions into the delay lines, unaffected by the
    // size parameter. This avoids 14 simultaneously-moving read heads
    // that create massive discontinuities when size changes.
    tap_l: [f32; 7],
    tap_r: [f32; 7],

    // DC blocking high-pass filters on the output (20 Hz cutoff).
    // Prevents DC offset accumulation in the feedback loop.
    // Standard DC blocker: y[n] = x[n] - x[n-1] + coeff * y[n-1]
    dc_prev_in_l: f32,
    dc_prev_in_r: f32,
    dc_prev_out_l: f32,
    dc_prev_out_r: f32,
    dc_block_coeff: f32,

    // Cached sample rate for parameter mapping
    sample_rate: f32,
}

// ─── Module ──────────────────────────────────────────────────────────────────

/// Stereo plate reverb based on the Dattorro algorithm.
///
/// Implements Jon Dattorro's plate reverberator with input diffusion,
/// a cross-coupled stereo tank, and multi-tap output. Even input
/// channels are summed to the left input, odd channels to the right.
/// Output is always 100% wet.
///
/// ```js
/// $dattorro($saw('c3'), { decay: 3, damping: 1, size: 2 }).out()
/// ```
#[module(name = "$dattorro", channels = 2, has_init, args(input))]
pub struct Dattorro {
    outputs: DattorroOutputs,
    state: DattorroState,
    params: DattorroParams,
}

impl Dattorro {
    /// Allocate all delay lines based on the sample rate.
    /// Called once at construction time on the main thread.
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;

        // Use a generous size multiplier for allocation so that the size
        // param can scale delay lengths up at runtime without exceeding capacity.
        let max_size = 2.5;

        // Predelay: up to MAX_PREDELAY_SECS
        let max_predelay = (MAX_PREDELAY_SECS * sample_rate).ceil() as usize;
        self.state.predelay_l = DelayLine::new(max_predelay);
        self.state.predelay_r = DelayLine::new(max_predelay);

        // Input diffusers
        let input_diff_lengths = [INPUT_DIFF_1, INPUT_DIFF_2, INPUT_DIFF_3, INPUT_DIFF_4];
        for (i, &ref_len) in input_diff_lengths.iter().enumerate() {
            let max_len = scale_delay(ref_len, sample_rate, max_size);
            self.state.input_diff[i] = DelayLine::new(max_len.max(1));
        }

        // Tank
        self.state.decay_diff_l =
            DelayLine::new(scale_delay(DECAY_DIFF_1, sample_rate, max_size).max(1));
        self.state.tank_delay_l =
            DelayLine::new(scale_delay(TANK_DELAY_1, sample_rate, max_size).max(1));
        self.state.decay_diff_r =
            DelayLine::new(scale_delay(DECAY_DIFF_2, sample_rate, max_size).max(1));
        self.state.tank_delay_r =
            DelayLine::new(scale_delay(TANK_DELAY_2, sample_rate, max_size).max(1));

        // Cache output tap positions — scaled by sample rate only, NOT size.
        // Taps are fixed read offsets into the delay lines so they don't
        // move when size changes.
        let sr_scale = sample_rate / REF_SAMPLE_RATE;
        self.state.tap_l = [
            TAP_L1 * sr_scale,
            TAP_L2 * sr_scale,
            TAP_L3 * sr_scale,
            TAP_L4 * sr_scale,
            TAP_L5 * sr_scale,
            TAP_L6 * sr_scale,
            TAP_L7 * sr_scale,
        ];
        self.state.tap_r = [
            TAP_R1 * sr_scale,
            TAP_R2 * sr_scale,
            TAP_R3 * sr_scale,
            TAP_R4 * sr_scale,
            TAP_R5 * sr_scale,
            TAP_R6 * sr_scale,
            TAP_R7 * sr_scale,
        ];

        // DC blocking coefficient: one-pole HPF at ~20 Hz
        // coeff = 1 - (2π * fc / sr)
        let dc_fc = 20.0_f32;
        self.state.dc_block_coeff = 1.0 - (std::f32::consts::TAU * dc_fc / sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let sample_rate = self.state.sample_rate;
        let num_input_channels = self.params.input.channels();

        // ── Read parameters ──────────────────────────────────────────────

        // Map bipolar -5..5 to algorithm coefficients (clamped for safety)
        let decay_v = self.params.decay.value_or(0.0);
        let decay_coeff = map_range(decay_v, -5.0, 5.0, 0.0, 0.9995).clamp(0.0, 0.9995);

        let damp_v = self.params.damping.value_or(0.0);
        // Higher damping voltage = more damping = lower bandwidth
        let bandwidth = map_range(damp_v, -5.0, 5.0, 0.9999, 0.1).clamp(0.1, 0.9999);

        let size_v = self.params.size.value_or(0.0);
        let size_raw = map_range(size_v, -5.0, 5.0, 0.25, 2.0).clamp(0.25, 2.0);
        // Smooth the size parameter to prevent zipper noise.  Abrupt
        // changes to delay lengths inside the recirculating tank cause
        // discontinuities that get amplified by feedback on every iteration.
        self.state.smoothed_size.update(size_raw);
        let size = *self.state.smoothed_size;

        let predelay_secs = self
            .params
            .predelay
            .value_or(0.0)
            .clamp(0.0, MAX_PREDELAY_SECS);
        let predelay_raw = predelay_secs * sample_rate;
        self.state.smoothed_predelay.update(predelay_raw);
        let predelay_samples = *self.state.smoothed_predelay;

        let diff_v = self.params.diffusion.value_or(3.5);
        let input_diff_coeff = map_range(diff_v, 0.0, 5.0, 0.0, 0.75).clamp(0.0, 0.75);
        let decay_diff_1_coeff = map_range(diff_v, 0.0, 5.0, 0.0, 0.70).clamp(0.0, 0.70);
        self.state.smoothed_decay_diff_1.update(decay_diff_1_coeff);
        let decay_diff_1_coeff = *self.state.smoothed_decay_diff_1;

        let decay_diff_2_coeff = map_range(diff_v, 0.0, 5.0, 0.0, 0.50).clamp(0.0, 0.50);
        self.state.smoothed_decay_diff_2.update(decay_diff_2_coeff);
        let decay_diff_2_coeff = *self.state.smoothed_decay_diff_2;

        // Modulation: convert voltage to delay excursion in samples.
        // Same excursion applied to both L/R decay diffusion — the user can wire
        // different LFOs to separate $dattorro instances for quadrature stereo effects.
        let mod_v = self.params.modulation.value_or(0.0);
        let mod_excursion = mod_v * (REF_MOD_EXCURSION * sample_rate / REF_SAMPLE_RATE) / 5.0;

        // ── Sum input channels to stereo ─────────────────────────────────

        let mut left_in = 0.0f32;
        let mut right_in = 0.0f32;
        for ch in 0..num_input_channels {
            let sample = self.params.input.get_value(ch);
            if ch % 2 == 0 {
                left_in += sample;
            } else {
                right_in += sample;
            }
        }

        // ── Predelay ─────────────────────────────────────────────────────

        self.state.predelay_l.write(left_in);
        let left_predelayed = self.state.predelay_l.read_linear(predelay_samples);

        self.state.predelay_r.write(right_in);
        let right_predelayed = self.state.predelay_r.read_linear(predelay_samples);

        // Sum to mono for input diffusers
        let mono_in = (left_predelayed + right_predelayed) * 0.5;

        // ── Input diffusion (4 cascaded allpass filters) ─────────────────

        let diff_delays = [INPUT_DIFF_1, INPUT_DIFF_2, INPUT_DIFF_3, INPUT_DIFF_4];
        let mut diffused = mono_in;
        for (i, &ref_len) in diff_delays.iter().enumerate() {
            let delay = scale_delay_f(ref_len, sample_rate, size).max(1.0);
            diffused = self.state.input_diff[i].allpass_linear(diffused, delay, input_diff_coeff);
        }

        // ── Tank processing ──────────────────────────────────────────────

        // Left tank: input = diffused + right feedback
        let left_tank_in = diffused + self.state.feedback_r * decay_coeff;

        // Decay diffusion allpass (left) — fractional delay for modulation
        let dd_l_base = scale_delay_f(DECAY_DIFF_1, sample_rate, size);
        let dd_l_delay = (dd_l_base + mod_excursion).max(1.0);
        let left_after_ap =
            self.state
                .decay_diff_l
                .allpass_linear(left_tank_in, dd_l_delay, -decay_diff_1_coeff);

        // Delay line (left)
        self.state.tank_delay_l.write(left_after_ap);
        let td_l_delay = scale_delay_f(TANK_DELAY_1, sample_rate, size).max(1.0);
        let left_tank_out = self.state.tank_delay_l.read_linear(td_l_delay);

        // Damping (one-pole lowpass)
        self.state.damp_state_l =
            left_tank_out * bandwidth + self.state.damp_state_l * (1.0 - bandwidth);
        let left_damped = self.state.damp_state_l;

        // Right tank: input = diffused + left feedback
        let right_tank_in = diffused + self.state.feedback_l * decay_coeff;

        // Decay diffusion allpass (right) — fractional delay for modulation
        let dd_r_base = scale_delay_f(DECAY_DIFF_2, sample_rate, size);
        let dd_r_delay = (dd_r_base + mod_excursion).max(1.0);
        let right_after_ap =
            self.state
                .decay_diff_r
                .allpass_linear(right_tank_in, dd_r_delay, -decay_diff_2_coeff);

        // Delay line (right)
        self.state.tank_delay_r.write(right_after_ap);
        let td_r_delay = scale_delay_f(TANK_DELAY_2, sample_rate, size).max(1.0);
        let right_tank_out = self.state.tank_delay_r.read_linear(td_r_delay);

        // Damping (one-pole lowpass)
        self.state.damp_state_r =
            right_tank_out * bandwidth + self.state.damp_state_r * (1.0 - bandwidth);
        let right_damped = self.state.damp_state_r;

        // Store feedback (cross-coupled)
        self.state.feedback_l = left_damped * decay_coeff;
        self.state.feedback_r = right_damped * decay_coeff;

        // ── Output taps (fixed positions, sample-rate-scaled only) ─────
        //
        // Output tap positions are NOT scaled by the `size` parameter.
        // This is critical because scaling 14 tap positions simultaneously
        // by a changing `size` creates massive discontinuities in the
        // output sum.  The tank delay lengths already change with `size`
        // (affecting the reverb character), but the output observation
        // points stay fixed.
        let tl = &self.state.tap_l;
        let tr = &self.state.tap_r;

        let left_out = self.state.tank_delay_l.read_linear(tl[0])
            + self.state.tank_delay_l.read_linear(tl[1])
            - self.state.decay_diff_r.read_linear(tl[2])
            + self.state.tank_delay_r.read_linear(tl[3])
            - self.state.tank_delay_l.read_linear(tl[4])
            - self.state.decay_diff_l.read_linear(tl[5])
            - self.state.tank_delay_l.read_linear(tl[6]);

        let right_out = self.state.tank_delay_r.read_linear(tr[0])
            + self.state.tank_delay_r.read_linear(tr[1])
            - self.state.decay_diff_l.read_linear(tr[2])
            + self.state.tank_delay_l.read_linear(tr[3])
            - self.state.tank_delay_r.read_linear(tr[4])
            - self.state.decay_diff_r.read_linear(tr[5])
            - self.state.tank_delay_r.read_linear(tr[6]);

        // DC blocking (one-pole high-pass at ~20 Hz) — prevents DC offset
        // accumulation that the feedback loop can amplify.
        // y[n] = x[n] - x[n-1] + coeff * y[n-1]
        let c = self.state.dc_block_coeff;
        let left_ac = left_out - self.state.dc_prev_in_l + c * self.state.dc_prev_out_l;
        let right_ac = right_out - self.state.dc_prev_in_r + c * self.state.dc_prev_out_r;
        self.state.dc_prev_in_l = left_out;
        self.state.dc_prev_in_r = right_out;
        self.state.dc_prev_out_l = left_ac;
        self.state.dc_prev_out_r = right_ac;

        // Scale output (0.6 factor to prevent clipping with dense input)
        let output_gain = 0.6;
        self.outputs.sample.set(0, left_ac * output_gain);
        self.outputs.sample.set(1, right_ac * output_gain);
    }
}

message_handlers!(impl Dattorro {});

#[cfg(test)]
mod tests {
    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::types::Sampleable;
    use serde_json::json;
    use std::sync::Arc;

    const SAMPLE_RATE: f32 = 48000.0;
    const DEFAULT_PORT: &str = "output";

    fn make_dattorro(params: serde_json::Value) -> Box<dyn Sampleable> {
        let constructors = get_constructors();
        let deserializers = get_params_deserializers();
        let deserializer = deserializers.get("$dattorro").unwrap();
        let cached = deserializer(params).unwrap();
        let deserialized = DeserializedParams {
            params: cached.params,
            argument_spans: Default::default(),
            channel_count: cached.channel_count,
        };
        constructors.get("$dattorro").unwrap()(
            &"test-dattorro".to_string(),
            SAMPLE_RATE,
            deserialized,
        )
        .unwrap()
    }

    fn step(module: &dyn Sampleable) {
        module.tick();
        module.update();
    }

    fn collect_stereo(module: &dyn Sampleable, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        for _ in 0..n {
            step(module);
            let poly = module.get_poly_sample(DEFAULT_PORT).unwrap();
            left.push(poly.get(0));
            right.push(poly.get(1));
        }
        (left, right)
    }

    /// Helper to build params JSON with only `input` required; all others optional.
    fn dattorro_params(overrides: serde_json::Value) -> serde_json::Value {
        let mut base = json!({ "input": 0.0 });
        if let (Some(base_map), Some(over_map)) = (base.as_object_mut(), overrides.as_object()) {
            for (k, v) in over_map {
                base_map.insert(k.clone(), v.clone());
            }
        }
        base
    }

    #[test]
    fn works_with_only_input() {
        // All non-input params should be optional — construct with just input
        let dattorro = make_dattorro(json!({ "input": 1.0 }));
        let (left, right) = collect_stereo(dattorro.as_ref(), 5000);
        let left_energy: f32 = left.iter().map(|s| s * s).sum();
        let right_energy: f32 = right.iter().map(|s| s * s).sum();
        assert!(
            left_energy > 0.0,
            "should produce output with default params"
        );
        assert!(
            right_energy > 0.0,
            "should produce output with default params"
        );
    }

    #[test]
    fn silence_in_silence_out() {
        let dattorro = make_dattorro(dattorro_params(json!({})));
        let (left, right) = collect_stereo(dattorro.as_ref(), 1000);
        assert!(left.iter().all(|&s| s == 0.0), "left should be silent");
        assert!(right.iter().all(|&s| s == 0.0), "right should be silent");
    }

    #[test]
    fn impulse_produces_output() {
        let dattorro = make_dattorro(dattorro_params(json!({ "input": 1.0, "decay": 3.0 })));
        // Collect enough samples for the reverb tail to develop
        let (left, right) = collect_stereo(dattorro.as_ref(), 10000);

        // After the initial transient, there should be non-zero output
        let left_energy: f32 = left.iter().map(|s| s * s).sum();
        let right_energy: f32 = right.iter().map(|s| s * s).sum();
        assert!(
            left_energy > 0.0,
            "left channel should have energy from impulse"
        );
        assert!(
            right_energy > 0.0,
            "right channel should have energy from impulse"
        );
    }

    #[test]
    fn stereo_channels_differ() {
        // The Dattorro algorithm produces decorrelated stereo from different tap points
        let dattorro = make_dattorro(dattorro_params(
            json!({ "input": 1.0, "decay": 3.0, "size": 2.0 }),
        ));
        let (left, right) = collect_stereo(dattorro.as_ref(), 5000);

        // L and R should not be identical (stereo decorrelation)
        let identical = left
            .iter()
            .zip(right.iter())
            .all(|(l, r)| (l - r).abs() < 1e-10);
        assert!(!identical, "left and right channels should be decorrelated");
    }

    #[test]
    fn no_dc_offset_accumulation() {
        // Feed constant DC and check that output doesn't grow unbounded
        let dattorro = make_dattorro(dattorro_params(json!({ "input": 1.0, "decay": 2.0 })));
        let (left, right) = collect_stereo(dattorro.as_ref(), 48000); // 1 second

        // Check the last 1000 samples for DC offset stability
        let last_left = &left[47000..];
        let last_right = &right[47000..];
        let left_mean: f32 = last_left.iter().sum::<f32>() / last_left.len() as f32;
        let right_mean: f32 = last_right.iter().sum::<f32>() / last_right.len() as f32;

        // DC offset should be bounded (not growing)
        assert!(
            left_mean.abs() < 10.0,
            "left DC offset should be bounded, got: {left_mean}"
        );
        assert!(
            right_mean.abs() < 10.0,
            "right DC offset should be bounded, got: {right_mean}"
        );
    }

    #[test]
    fn higher_decay_produces_longer_tail() {
        // Compare energy with low vs high decay
        let dattorro_low = make_dattorro(dattorro_params(json!({ "input": 1.0, "decay": -3.0 })));
        let dattorro_high = make_dattorro(dattorro_params(json!({ "input": 1.0, "decay": 3.0 })));

        let n = 20000;
        let (left_low, _) = collect_stereo(dattorro_low.as_ref(), n);
        let (left_high, _) = collect_stereo(dattorro_high.as_ref(), n);

        // Measure energy in the tail (last quarter)
        let tail_start = n * 3 / 4;
        let low_tail_energy: f32 = left_low[tail_start..].iter().map(|s| s * s).sum();
        let high_tail_energy: f32 = left_high[tail_start..].iter().map(|s| s * s).sum();

        assert!(
            high_tail_energy > low_tail_energy,
            "higher decay should have more tail energy: high={high_tail_energy}, low={low_tail_energy}"
        );
    }

    #[test]
    fn output_is_two_channels() {
        let dattorro = make_dattorro(dattorro_params(json!({})));
        step(dattorro.as_ref());
        let poly = dattorro.get_poly_sample(DEFAULT_PORT).unwrap();
        assert_eq!(poly.channels(), 2, "output should be stereo (2 channels)");
    }

    #[test]
    fn modulation_changes_output() {
        // A dattorro with constant modulation offset should produce different output
        // than one without, confirming the modulation path is active.
        let n = 10000;

        let dattorro_no_mod = make_dattorro(dattorro_params(json!({ "input": 1.0, "decay": 3.0 })));
        let (left_no_mod, _) = collect_stereo(dattorro_no_mod.as_ref(), n);

        let dattorro_with_mod = make_dattorro(dattorro_params(
            json!({ "input": 1.0, "decay": 3.0, "modulation": 2.5 }),
        ));
        let (left_with_mod, _) = collect_stereo(dattorro_with_mod.as_ref(), n);

        // Outputs should differ due to modulated delay lengths
        let differs = left_no_mod
            .iter()
            .zip(left_with_mod.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(
            differs,
            "modulated dattorro should produce different output than unmodulated"
        );
    }
}
