//! Feedback Delay Network (FDN) reverb module.
//!
//! Two coupled sub-networks of parallel delay lines — one per stereo channel —
//! recirculate through an orthonormal mixing matrix, producing a dense, smooth
//! reverb tail that tracks the input's stereo image. Energy is conserved by the
//! matrix and bled off only by per-line decay gains and damping, so the network
//! is unconditionally stable for any finite decay time.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::dc_blocker::{DEFAULT_DC_BLOCK_FC_HZ, DcBlocker};
use crate::dsp::utils::delay_line::DelayLine;
use crate::dsp::utils::map_range;
use crate::poly::{MonoSignal, MonoSignalExt, PolyOutput, PolySignal};
use crate::types::Clickless;

// ─── Topology ────────────────────────────────────────────────────────────────

/// Total delay-line count across both channels.
const NUM_LINES: usize = 8;

/// Delay lines per channel. The first `HALF_LINES` lines form the left
/// sub-network, the rest the right. Must stay a power of two — each block is
/// mixed with a fast Walsh-Hadamard transform that assumes it.
const HALF_LINES: usize = 4;

/// Input-diffusion allpass count, per channel.
const NUM_DIFFUSERS: usize = 4;

/// Delay lengths are authored as sample counts at this reference rate and
/// rescaled to the engine's actual rate at construction.
const REF_SAMPLE_RATE: f32 = 48000.0;

/// FDN delay lengths (samples at the reference rate). Pairwise-coprime primes;
/// the first four (left block) and last four (right block) each span the full
/// ~24–70 ms range, so the two channels share a matched character while staying
/// decorrelated.
const FDN_DELAYS: [f32; NUM_LINES] = [
    1153.0, 1567.0, 2129.0, 2887.0, // left block
    1327.0, 1823.0, 2477.0, 3361.0, // right block
];

/// Input-diffusion allpass lengths (samples at the reference rate). Short
/// coprime primes that smear the dry transient before it enters the network.
const DIFFUSER_DELAYS: [f32; NUM_DIFFUSERS] = [113.0, 173.0, 271.0, 421.0];

/// Input-diffusion allpass coefficient. Fixed — diffusion density is a
/// character choice, not a user control.
const DIFFUSER_COEFF: f32 = 0.7;

/// Per-line modulation LFO rates (Hz). Mutually detuned and slow, so the
/// chorusing decorrelates the lines without audible pitch wobble.
const LFO_RATES_HZ: [f32; NUM_LINES] = [0.53, 0.79, 1.03, 1.21, 0.67, 0.91, 1.13, 1.31];

/// `1 / sqrt(HALF_LINES)` — normalizes each block's Hadamard mix to be
/// orthonormal (energy-preserving), which keeps the feedback loop stable.
const HADAMARD4_NORM: f32 = 0.5; // 1 / sqrt(4)

// ─── Parameter ranges ─────────────────────────────────────────────────────────

const MIN_SIZE: f32 = 0.5;
const MAX_SIZE: f32 = 2.0;
const MAX_PREDELAY_SECS: f32 = 0.5;
/// Peak modulation excursion (samples at the reference rate) at full depth.
const MAX_MOD_EXCURSION: f32 = 12.0;
const MIN_T60: f32 = 0.15;
const MAX_T60: f32 = 18.0;
/// Per-line gains are clamped below 1.0 so the loop can never become lossless
/// even at the maximum decay time.
const MAX_FEEDBACK: f32 = 0.9995;
/// `ln(1000)` — the −60 dB factor in the Jot decay-gain formula.
const LN_1000: f32 = 6.907_755;

/// Cross-coupling rotation angle between the two channels' sub-networks at the
/// widest (`+5`) and narrowest (`-5`) settings. A small angle keeps the halves
/// separate so the wet field holds the input's pan; 45° maximally merges them,
/// pulling a panned source toward the centre.
const WIDE_ANGLE: f32 = 0.104_72; // ~6°
const NARROW_ANGLE: f32 = std::f32::consts::FRAC_PI_4; // 45°

const INPUT_GAIN: f32 = 0.5;
/// Scales the output taps only — outside the feedback loop, so it sets the wet
/// level without affecting stability. Tuned to sit alongside the other reverbs.
const OUTPUT_GAIN: f32 = 1.15;

// Per-block sign patterns: the input pattern spreads the dry signal across a
// block's modes; the tap pattern reads a decorrelated combination back out.
const INPUT_SIGNS: [f32; HALF_LINES] = [1.0, -1.0, 1.0, -1.0];
const TAP_SIGNS: [f32; HALF_LINES] = [1.0, 1.0, -1.0, -1.0];

/// Apply the orthonormal 4-point Walsh-Hadamard transform in place. Adds,
/// subtracts, and one final scale — no allocation.
#[inline]
fn hadamard4(v: &mut [f32; HALF_LINES]) {
    let a = v[0] + v[1];
    let b = v[0] - v[1];
    let c = v[2] + v[3];
    let d = v[2] - v[3];
    v[0] = (a + c) * HADAMARD4_NORM;
    v[1] = (b + d) * HADAMARD4_NORM;
    v[2] = (a - c) * HADAMARD4_NORM;
    v[3] = (b - d) * HADAMARD4_NORM;
}

/// One-pole lowpass step with unity DC gain: `y = c·x + (1 − c)·y_prev`. Keeping
/// DC gain at unity means damping shortens only the high-frequency decay,
/// leaving the loop's low-frequency gain (and thus stability) untouched.
#[inline]
fn one_pole(prev: f32, x: f32, coeff: f32) -> f32 {
    prev + coeff * (x - prev)
}

// ─── Params ──────────────────────────────────────────────────────────────────

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ReverbParams {
    /// audio input — even channels feed the left half of the reverb, odd
    /// channels the right; an odd channel count feeds the last channel to both
    input: PolySignal,
    /// reverb decay time, mapped logarithmically from ~0.15 s (-5) to ~18 s
    /// (+5). default 0 → ~1.6 s
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    decay: Option<MonoSignal>,
    /// high-frequency damping — higher values darken the tail by absorbing more
    /// treble on each recirculation. default 0
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    damping: Option<MonoSignal>,
    /// room size — scales every delay length from 0.5× (-5) to 2× (+5),
    /// trading echo spacing and modal density. default 0 → 1.25×
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    size: Option<MonoSignal>,
    /// stereo width — how strongly the left and right halves of the network
    /// stay separate. -5 is narrowest (a panned source pulls toward centre),
    /// +5 is widest; default 0 → natural
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    width: Option<MonoSignal>,
    /// predelay before the network, in seconds (0 to 0.5). default 0
    #[signal(default = 0.0, range = (0.0, 0.5))]
    #[deserr(default)]
    predelay: Option<MonoSignal>,
    /// internal chorusing depth — subtly detunes the delay lines to reduce
    /// metallic ringing. -5 is off, +5 is deepest; default 0 → gentle
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    modulation: Option<MonoSignal>,
}

// ─── Outputs ─────────────────────────────────────────────────────────────────

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ReverbOutputs {
    #[output("output", "stereo reverb output (ch0=left, ch1=right)", default)]
    sample: PolyOutput,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Pre-allocated state. Every `DelayLine` defaults to empty and is sized in
/// `init()`; runtime state (filter memories, LFO phases) carries across patch
/// updates so the tail never glitches on an edit.
#[derive(Default)]
struct ReverbState {
    // Per-channel input sections.
    predelay_l: DelayLine,
    predelay_r: DelayLine,
    diffusers_l: [DelayLine; NUM_DIFFUSERS],
    diffusers_r: [DelayLine; NUM_DIFFUSERS],

    // Feedback delay network (left block then right block).
    lines: [DelayLine; NUM_LINES],
    /// Per-line damping lowpass memory.
    damp_state: [f32; NUM_LINES],
    /// Per-line modulation LFO phase, in turns `[0, 1)`.
    lfo_phase: [f32; NUM_LINES],

    // DC blocking on the output — stops the feedback loop from integrating any
    // residual offset in the dry signal.
    dc_blocker_l: DcBlocker,
    dc_blocker_r: DcBlocker,
    dc_block_coeff: f32,

    // Parameter smoothing. These all feed the recirculating loop, where a step
    // change recirculates as an audible transient, so they are slewed.
    smoothed_size: Clickless,
    smoothed_predelay: Clickless,
    smoothed_decay_k: Clickless,
    smoothed_damp: Clickless,
    /// Smoothed cross-coupling angle (radians).
    smoothed_width: Clickless,

    sample_rate: f32,

    // Fixed read lengths, derived from the sample rate only (so constant for
    // the module's lifetime) — the size-1.0 delay in samples for each line.
    base_len: [f32; NUM_LINES],
    diffuser_len: [f32; NUM_DIFFUSERS],
    /// Upper clamp on each line's read position, kept inside the allocated
    /// buffer even at maximum size plus full modulation excursion.
    max_read: [f32; NUM_LINES],
    /// Per-sample LFO phase increment per line (sample-rate-only).
    lfo_inc: [f32; NUM_LINES],
    /// Peak modulation excursion in samples, reached at maximum depth.
    max_mod_excursion: f32,
}

// ─── Module ──────────────────────────────────────────────────────────────────

/// Stereo Feedback Delay Network reverb.
///
/// The eight modulated delay lines are split into a left and a right
/// sub-network. Even input channels feed the left half, odd channels the right,
/// so the wet field tracks the dry stereo image; a `width` control sets how
/// strongly the two halves cross-couple, from held-apart to collapsed to mono.
/// Each line has a decay gain tuned so the network reaches −60 dB at the
/// requested `decay`, plus a one-pole damping filter for a natural
/// high-frequency rolloff. Per-channel predelay and diffusion sit ahead of the
/// network. Output is always 100% wet — use `.send()` or `$mix` for a dry/wet
/// blend.
///
/// ```js
/// $unstable.reverb($saw('c3')).out()
/// $unstable.reverb($saw('c3'), { decay: 3, damping: 1.5, size: 2 }).out()
/// $unstable.reverb($saw('c3'), { decay: 4, width: 4, modulation: 3 }).out()
/// ```
#[module(name = "$unstable.reverb", channels = 2, has_init, args(input))]
pub struct Reverb {
    outputs: ReverbOutputs,
    state: ReverbState,
    params: ReverbParams,
}

impl Reverb {
    /// Allocate the delay lines and cache every sample-rate-only constant.
    /// Runs once at construction on the main thread, where allocation is legal.
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;

        let sr_scale = sample_rate / REF_SAMPLE_RATE;
        let max_mod_samples = MAX_MOD_EXCURSION * sr_scale;
        self.state.max_mod_excursion = max_mod_samples;

        // Predelay: one per channel, sized to the maximum predelay time.
        let max_predelay = (MAX_PREDELAY_SECS * sample_rate).ceil() as usize + 4;
        self.state.predelay_l = DelayLine::new(max_predelay);
        self.state.predelay_r = DelayLine::new(max_predelay);

        // Input diffusers: one chain per channel, sized for the largest room.
        for i in 0..NUM_DIFFUSERS {
            let base = DIFFUSER_DELAYS[i] * sr_scale;
            self.state.diffuser_len[i] = base;
            let cap = (base * MAX_SIZE).ceil() as usize + 4;
            self.state.diffusers_l[i] = DelayLine::new(cap.max(1));
            self.state.diffusers_r[i] = DelayLine::new(cap.max(1));
        }

        // FDN lines: sized for the largest room plus full modulation excursion,
        // so the modulated read position can never run past the buffer.
        let golden = 0.618_034_f32;
        for i in 0..NUM_LINES {
            let base = FDN_DELAYS[i] * sr_scale;
            self.state.base_len[i] = base;
            self.state.max_read[i] = base * MAX_SIZE + max_mod_samples + 1.0;
            let cap = (base * MAX_SIZE).ceil() as usize + max_mod_samples.ceil() as usize + 4;
            self.state.lines[i] = DelayLine::new(cap.max(1));

            // Golden-ratio-spaced initial phases decorrelate the LFOs from the
            // first sample; the distinct rates keep them decorrelated after.
            self.state.lfo_phase[i] = (i as f32 * golden).fract();
            self.state.lfo_inc[i] = LFO_RATES_HZ[i] / sample_rate;
        }

        self.state.dc_block_coeff = DcBlocker::coeff(DEFAULT_DC_BLOCK_FC_HZ, sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let num_input_channels = self.params.input.channels();

        // ── Read and smooth parameters ───────────────────────────────────

        let size_raw = map_range(
            self.params.size.value_or(0.0),
            -5.0,
            5.0,
            MIN_SIZE,
            MAX_SIZE,
        )
        .clamp(MIN_SIZE, MAX_SIZE);
        self.state.smoothed_size.update(size_raw);
        let size = *self.state.smoothed_size;

        let predelay_secs = self
            .params
            .predelay
            .value_or(0.0)
            .clamp(0.0, MAX_PREDELAY_SECS);
        self.state
            .smoothed_predelay
            .update(predelay_secs * self.state.sample_rate);
        let predelay_samples = *self.state.smoothed_predelay;

        // Decay maps to a target T60, then to the per-sample-length gain
        // exponent `k` where each line's gain is `exp(k · length)`.
        let decay_v = self.params.decay.value_or(0.0);
        let t60 = map_range(decay_v, -5.0, 5.0, MIN_T60.ln(), MAX_T60.ln())
            .exp()
            .clamp(MIN_T60, MAX_T60);
        let decay_k = -LN_1000 / (self.state.sample_rate * t60);
        self.state.smoothed_decay_k.update(decay_k);
        let decay_k = *self.state.smoothed_decay_k;

        // Damping: more voltage → lower coefficient → darker tail.
        let damp_coeff =
            map_range(self.params.damping.value_or(0.0), -5.0, 5.0, 1.0, 0.08).clamp(0.08, 1.0);
        self.state.smoothed_damp.update(damp_coeff);
        let damp_coeff = *self.state.smoothed_damp;

        // Width sets the cross-coupling rotation between the two sub-networks:
        // +5 → a shallow angle that holds the channels apart, -5 → 45° that
        // fully merges them. Smoothing the angle keeps `sin²+cos²=1` exact.
        let angle_raw = map_range(
            self.params.width.value_or(0.0),
            -5.0,
            5.0,
            NARROW_ANGLE,
            WIDE_ANGLE,
        )
        .clamp(WIDE_ANGLE, NARROW_ANGLE);
        self.state.smoothed_width.update(angle_raw);
        let (cross_sin, cross_cos) = (*self.state.smoothed_width).sin_cos();

        let mod_depth = map_range(
            self.params.modulation.value_or(0.0).clamp(-5.0, 5.0),
            -5.0,
            5.0,
            0.0,
            self.state.max_mod_excursion,
        );

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
        // With an odd channel count the final channel has no stereo partner,
        // so feed it to both sides (a mono input drives left and right equally).
        if num_input_channels % 2 == 1 {
            right_in += self.params.input.get_value(num_input_channels - 1);
        }

        // ── Per-channel predelay then input diffusion ────────────────────

        self.state.predelay_l.write(left_in);
        self.state.predelay_r.write(right_in);
        let mut diff_l = self.state.predelay_l.read_linear(predelay_samples);
        let mut diff_r = self.state.predelay_r.read_linear(predelay_samples);
        for i in 0..NUM_DIFFUSERS {
            let delay = (self.state.diffuser_len[i] * size).max(1.0);
            diff_l = self.state.diffusers_l[i].allpass_linear(diff_l, delay, DIFFUSER_COEFF);
            diff_r = self.state.diffusers_r[i].allpass_linear(diff_r, delay, DIFFUSER_COEFF);
        }
        let inject_l = diff_l * INPUT_GAIN;
        let inject_r = diff_r * INPUT_GAIN;

        // ── Read the network, damp, attenuate ────────────────────────────

        let mut returns = [0.0f32; NUM_LINES];
        let mut left_out = 0.0f32;
        let mut right_out = 0.0f32;
        for i in 0..NUM_LINES {
            let length = self.state.base_len[i] * size;

            // Advance this line's LFO and read at the (fractional) modulated tap.
            let mut phase = self.state.lfo_phase[i] + self.state.lfo_inc[i];
            if phase >= 1.0 {
                phase -= 1.0;
            }
            self.state.lfo_phase[i] = phase;
            let excursion = mod_depth * (std::f32::consts::TAU * phase).sin();
            let read_len = (length + excursion).clamp(1.0, self.state.max_read[i]);
            let delayed = self.state.lines[i].read_linear(read_len);

            // The left block feeds the left output tap, the right block the
            // right, so a panned source stays panned in the wet field.
            if i < HALF_LINES {
                left_out += delayed * TAP_SIGNS[i];
            } else {
                right_out += delayed * TAP_SIGNS[i - HALF_LINES];
            }

            // Damp, then attenuate by the nominal-length decay gain.
            let damped = one_pole(self.state.damp_state[i], delayed, damp_coeff);
            self.state.damp_state[i] = damped;
            let gain = (decay_k * length).exp().min(MAX_FEEDBACK);
            returns[i] = damped * gain;
        }

        // ── Mix each block, cross-couple, write back with stereo input ───

        let mut left_block = [returns[0], returns[1], returns[2], returns[3]];
        let mut right_block = [returns[4], returns[5], returns[6], returns[7]];
        hadamard4(&mut left_block);
        hadamard4(&mut right_block);
        for i in 0..HALF_LINES {
            // Rotate each paired (left, right) line by the cross-coupling angle.
            // A rotation is orthonormal, so the full matrix stays energy-preserving.
            let l = cross_cos * left_block[i] + cross_sin * right_block[i];
            let r = cross_cos * right_block[i] - cross_sin * left_block[i];
            self.state.lines[i].write(inject_l * INPUT_SIGNS[i] + l);
            self.state.lines[i + HALF_LINES].write(inject_r * INPUT_SIGNS[i] + r);
        }

        // ── Level, DC-block, emit ────────────────────────────────────────

        let coeff = self.state.dc_block_coeff;
        let out_l = self
            .state
            .dc_blocker_l
            .process(left_out * OUTPUT_GAIN, coeff);
        let out_r = self
            .state
            .dc_blocker_r
            .process(right_out * OUTPUT_GAIN, coeff);

        self.outputs.sample.set(0, out_l);
        self.outputs.sample.set(1, out_r);
    }
}

message_handlers!(impl Reverb {});

#[cfg(test)]
mod tests {
    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::types::Sampleable;
    use serde_json::json;

    const SAMPLE_RATE: f32 = 48000.0;
    const DEFAULT_PORT: &str = "output";
    const TEST_BLOCK_SIZE: usize = 1;

    fn make_reverb(params: serde_json::Value) -> Box<dyn Sampleable> {
        make_reverb_at(params, SAMPLE_RATE)
    }

    fn make_reverb_at(params: serde_json::Value, sample_rate: f32) -> Box<dyn Sampleable> {
        let constructors = get_constructors();
        let deserializers = get_params_deserializers();
        let deserializer = deserializers.get("$unstable.reverb").unwrap();
        let cached = deserializer(params).unwrap();
        let deserialized = DeserializedParams {
            params: cached.params,
            channel_count: cached.channel_count,
        };
        constructors.get("$unstable.reverb").unwrap()(
            &"test-reverb".to_string(),
            sample_rate,
            deserialized,
            TEST_BLOCK_SIZE,
            crate::types::ProcessingMode::Block,
        )
        .unwrap()
    }

    fn collect_stereo(module: &dyn Sampleable, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        let mut produced = 0;
        while produced < n {
            module.start_block();
            module.ensure_processed();
            let take = TEST_BLOCK_SIZE.min(n - produced);
            for slot in 0..take {
                left.push(module.get_value_at(DEFAULT_PORT, 0, slot));
                right.push(module.get_value_at(DEFAULT_PORT, 1, slot));
            }
            produced += take;
        }
        (left, right)
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s * s).sum()
    }

    /// Build params JSON with only `input` required; all others optional.
    fn reverb_params(overrides: serde_json::Value) -> serde_json::Value {
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
        let reverb = make_reverb(json!({ "input": 1.0 }));
        let (left, right) = collect_stereo(reverb.as_ref(), 10000);
        assert!(
            energy(&left) > 0.0,
            "should produce output with default params"
        );
        assert!(
            energy(&right) > 0.0,
            "should produce output with default params"
        );
    }

    #[test]
    fn silence_in_silence_out() {
        let reverb = make_reverb(reverb_params(json!({})));
        let (left, right) = collect_stereo(reverb.as_ref(), 1000);
        assert!(left.iter().all(|&s| s == 0.0), "left should be silent");
        assert!(right.iter().all(|&s| s == 0.0), "right should be silent");
    }

    #[test]
    fn impulse_produces_output() {
        let reverb = make_reverb(reverb_params(json!({ "input": 1.0, "decay": 3.0 })));
        let (left, right) = collect_stereo(reverb.as_ref(), 20000);
        assert!(
            energy(&left) > 0.0,
            "left channel should have energy from impulse"
        );
        assert!(
            energy(&right) > 0.0,
            "right channel should have energy from impulse"
        );
    }

    #[test]
    fn stereo_channels_differ() {
        let reverb = make_reverb(reverb_params(json!({ "input": 1.0, "decay": 3.0 })));
        let (left, right) = collect_stereo(reverb.as_ref(), 10000);
        let identical = left
            .iter()
            .zip(right.iter())
            .all(|(l, r)| (l - r).abs() < 1e-10);
        assert!(!identical, "left and right channels should be decorrelated");
    }

    #[test]
    fn preserves_input_pan() {
        // A hard-left dry input should reverberate to a left-dominant wet field.
        let reverb = make_reverb(reverb_params(
            json!({ "input": [1.0, 0.0], "decay": 2.0, "width": 4.0 }),
        ));
        let (left, right) = collect_stereo(reverb.as_ref(), 20000);
        assert!(
            energy(&left) > energy(&right) * 1.3,
            "left-panned input should bias the wet field left: L={}, R={}",
            energy(&left),
            energy(&right)
        );
    }

    #[test]
    fn width_controls_stereo_spread() {
        // A wider setting should keep more of a panned source on its own side
        // than a narrow (toward-mono) setting.
        let n = 20000;
        let wide = make_reverb(reverb_params(
            json!({ "input": [1.0, 0.0], "decay": 2.0, "width": 5.0 }),
        ));
        let (wl, wr) = collect_stereo(wide.as_ref(), n);
        let narrow = make_reverb(reverb_params(
            json!({ "input": [1.0, 0.0], "decay": 2.0, "width": -5.0 }),
        ));
        let (nl, nr) = collect_stereo(narrow.as_ref(), n);
        let wide_bias = energy(&wl) / energy(&wr).max(1e-9);
        let narrow_bias = energy(&nl) / energy(&nr).max(1e-9);
        assert!(
            wide_bias > narrow_bias,
            "wider setting should bias more toward the panned side: wide={wide_bias}, narrow={narrow_bias}"
        );
    }

    #[test]
    fn output_stays_finite_and_bounded() {
        // Sustained full-scale DC is the worst case for a feedback loop:
        // the matrix conserves energy and only the sub-unity line gains and
        // DC blocker keep it in check.
        let reverb = make_reverb(reverb_params(json!({ "input": 1.0, "decay": 2.0 })));
        let (left, right) = collect_stereo(reverb.as_ref(), 48000);
        assert!(
            left.iter().chain(right.iter()).all(|s| s.is_finite()),
            "output must never go non-finite"
        );
        let last_left = &left[47000..];
        let left_mean: f32 = last_left.iter().sum::<f32>() / last_left.len() as f32;
        assert!(
            left_mean.abs() < 10.0,
            "left DC offset should be bounded, got: {left_mean}"
        );
    }

    #[test]
    fn higher_decay_produces_longer_tail() {
        let reverb_low = make_reverb(reverb_params(json!({ "input": 1.0, "decay": -3.0 })));
        let reverb_high = make_reverb(reverb_params(json!({ "input": 1.0, "decay": 3.0 })));
        let n = 40000;
        let (left_low, _) = collect_stereo(reverb_low.as_ref(), n);
        let (left_high, _) = collect_stereo(reverb_high.as_ref(), n);
        let tail_start = n * 3 / 4;
        let low_tail_energy = energy(&left_low[tail_start..]);
        let high_tail_energy = energy(&left_high[tail_start..]);
        assert!(
            high_tail_energy > low_tail_energy,
            "higher decay should have more tail energy: high={high_tail_energy}, low={low_tail_energy}"
        );
    }

    #[test]
    fn modulation_changes_output() {
        let n = 20000;
        let reverb_off = make_reverb(reverb_params(
            json!({ "input": 1.0, "decay": 3.0, "modulation": -5.0 }),
        ));
        let (left_off, _) = collect_stereo(reverb_off.as_ref(), n);
        let reverb_deep = make_reverb(reverb_params(
            json!({ "input": 1.0, "decay": 3.0, "modulation": 5.0 }),
        ));
        let (left_deep, _) = collect_stereo(reverb_deep.as_ref(), n);
        let differs = left_off
            .iter()
            .zip(left_deep.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(differs, "deep modulation should differ from modulation off");
    }

    #[test]
    fn damping_changes_output() {
        let n = 10000;
        let reverb_bright = make_reverb(reverb_params(json!({ "input": 1.0, "damping": -5.0 })));
        let reverb_dark = make_reverb(reverb_params(json!({ "input": 1.0, "damping": 5.0 })));
        let (left_bright, _) = collect_stereo(reverb_bright.as_ref(), n);
        let (left_dark, _) = collect_stereo(reverb_dark.as_ref(), n);
        let differs = left_bright
            .iter()
            .zip(left_dark.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(differs, "different damping should produce different output");
    }

    #[test]
    fn max_size_and_modulation_stays_bounded() {
        // Largest room plus deepest modulation pushes every line's read tap to
        // its furthest point. The buffers are sized for exactly this corner;
        // an undersized buffer would wrap and read stale samples back into the
        // feedback loop rather than panic, so guard the bound by driving it.
        let reverb = make_reverb(reverb_params(json!({
            "input": 1.0,
            "decay": 5.0,
            "size": 5.0,
            "modulation": 5.0,
        })));
        let (left, right) = collect_stereo(reverb.as_ref(), 96000);
        assert!(
            left.iter().chain(right.iter()).all(|s| s.is_finite()),
            "output must stay finite at maximum size and modulation"
        );
        let peak = left
            .iter()
            .chain(right.iter())
            .fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak < 50.0, "output should stay bounded, got peak {peak}");
    }

    #[test]
    fn stable_at_non_48k_sample_rates() {
        // The other tests build at 48 kHz where the reference-rate scale is
        // exactly 1.0; drive the worst-case buffer corner at fractional-scale
        // rates so the delay-line sizing (ceil/round of scaled lengths) is
        // exercised where a rounding slip could read past a buffer.
        for &sr in &[44100.0f32, 96000.0] {
            let reverb = make_reverb_at(
                reverb_params(json!({
                    "input": 1.0,
                    "decay": 5.0,
                    "size": 5.0,
                    "modulation": 5.0,
                })),
                sr,
            );
            let (left, right) = collect_stereo(reverb.as_ref(), 96000);
            assert!(
                left.iter().chain(right.iter()).all(|s| s.is_finite()),
                "output must stay finite at {sr} Hz"
            );
            let peak = left
                .iter()
                .chain(right.iter())
                .fold(0.0f32, |m, &s| m.max(s.abs()));
            assert!(
                peak < 50.0,
                "output should stay bounded at {sr} Hz, got peak {peak}"
            );
        }
    }
}
