//! Single-band feed-forward compressor module.
//!
//! Peak-detecting compressor with configurable threshold, ratio,
//! attack/release times, makeup gain, and input/output gain staging.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::sanitize;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt};

// Gain voltage scaling: maps [-5, 5] volts to [-24, 24] dB (4.8 dB per volt)
const DB_PER_VOLT: f32 = 4.8;

/// Convert a bipolar voltage (-5 to +5) to a linear gain multiplier.
/// 0V = 0dB (unity), -5V = -24dB, +5V = +24dB.
#[inline]
fn voltage_to_gain(voltage: f32) -> f32 {
    let db = voltage.clamp(-5.0, 5.0) * DB_PER_VOLT;
    10.0_f32.powf(db / 20.0)
}

/// Compute compressor gain for a single sample.
///
/// Envelope is updated from `detector_sample` (the side-chain signal — for
/// internal detection this is the same as `sample`). Two stages run on every
/// sample: a downward stage active above `threshold` and an upward stage
/// active below `upward_threshold`. Each stage's `ratio` is interpreted via
/// the gain factor `1 − 1/ratio`:
///
/// - `ratio == 1` → factor 0 → passthrough.
/// - `ratio > 1` → positive factor → compression (pull toward threshold).
/// - `ratio < 1` → negative factor → expansion (push away from threshold).
///
/// To keep expansion bounded at signal extremes, `level_db` is clamped to
/// `[-60, +60]` dB before the gain math.
#[inline]
fn compress(
    sample: f32,
    detector_sample: f32,
    envelope: &mut f32,
    threshold: f32,
    ratio: f32,
    upward_threshold: f32,
    upward_ratio: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    sample_rate: f32,
) -> f32 {
    // Lower-bound the ratios at 1e-3 so `1/ratio` stays finite. The gain math
    // is shaped by `(1 - 1/ratio)`: ratio ∈ (0, 1) is expansion, ratio == 1 is
    // passthrough, ratio > 1 is compression. The ±60 dB clamp on `level_db`
    // below is what actually keeps the output bounded at extreme ratios.
    let ratio = ratio.max(1e-3);
    let upward_ratio = upward_ratio.max(1e-3);
    let threshold = threshold.max(0.0);
    let upward_threshold = upward_threshold.max(0.0);
    let attack = attack.max(1e-6);
    let release = release.max(1e-6);

    // Envelope follower (peak detection with attack/release ballistics)
    let detector_abs = detector_sample.abs();
    let coeff = if detector_abs > *envelope {
        (-1.0 / (attack * sample_rate)).exp()
    } else {
        (-1.0 / (release * sample_rate)).exp()
    };
    *envelope = detector_abs + coeff * (*envelope - detector_abs);
    *envelope = sanitize(*envelope);

    // Gain computation in dB domain. Clamp the detected level to ±60 dB so
    // expansion (negative gain factor) stays bounded at extreme inputs:
    // without the floor, silence drives upward expansion to +∞ dB; without
    // the ceiling, very loud peaks drive downward expansion the same way.
    let level_db = (20.0 * (*envelope + 1e-10).log10()).clamp(-60.0, 60.0);
    let threshold_db = 20.0 * (threshold + 1e-10).log10();
    let upward_threshold_db = 20.0 * (upward_threshold + 1e-10).log10();

    // Downward stage (above threshold): compress when ratio > 1, expand when
    // ratio < 1. Factor is 0 at ratio == 1 → passthrough, no branch needed.
    let down_db = if level_db > threshold_db {
        (threshold_db - level_db) * (1.0 - 1.0 / ratio)
    } else {
        0.0
    };

    // Upward stage (below threshold): compress (boost quiet signal) when
    // ratio > 1, expand (gate-like attenuation of quiet signal) when
    // ratio < 1.
    let up_db = if level_db < upward_threshold_db {
        (upward_threshold_db - level_db) * (1.0 - 1.0 / upward_ratio)
    } else {
        0.0
    };

    // Clamp the final gain to ±60 dB. The level clamp alone isn't enough at
    // extreme expansion ratios — factor `1 - 1/ratio` is unbounded (e.g.
    // ratio = 1e-3 → factor ≈ -1000), and even a bounded level_db produces
    // hundreds of dB of swing which overflows when raised back to linear.
    let gain_db = (down_db + up_db).clamp(-60.0, 60.0);
    let gain = 10.0_f32.powf(gain_db / 20.0);

    sample * gain * makeup
}

#[derive(Clone, Copy, Default)]
struct ChannelState {
    envelope: f32,
}

/// State for the Compressor module.
#[derive(Default)]
struct CompressorState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct CompressorParams {
    /// audio input signal
    input: PolySignal,
    /// optional side-chain detector input — when connected, the envelope follower
    /// reads from this signal instead of `input`, but the gain is still applied
    /// to `input`. Use for ducking, kick-triggered pumping, etc.
    #[deserr(default)]
    sidechain: Option<PolySignal>,
    /// downward stage threshold (0-5V, default 2.5)
    #[deserr(default)]
    threshold: Option<PolySignal>,
    /// downward stage ratio in textbook X:1 form (= Δinput dB / Δoutput dB above
    /// threshold). > 1 compresses, < 1 expands (boost loud), 1 = passthrough.
    /// `f32::INFINITY` acts as a brick-wall limiter (output clamps to
    /// threshold). Equivalent to Ableton's 1:Y display via `ratio = 1/Y`.
    /// Lower-bounded at 1e-3. Default 4.0 (= Ableton 1:0.25).
    #[deserr(default)]
    ratio: Option<PolySignal>,
    /// upward stage threshold — signal *below* this gets pushed toward the
    /// threshold (0-5V, default 0 = bypassed; no real signal sits below the
    /// resulting ~-200 dB floor)
    #[deserr(default)]
    upward_threshold: Option<PolySignal>,
    /// upward stage ratio in textbook X:1 form. > 1 compresses below threshold
    /// (boosts quiet signal), < 1 expands below threshold (gates quiet signal),
    /// 1 = passthrough. Equivalent to Ableton's 1:Y display via `ratio = 1/Y`.
    /// Lower-bounded at 1e-3. Default 1.0.
    #[deserr(default)]
    upward_ratio: Option<PolySignal>,
    /// attack time in seconds (default 0.01)
    #[deserr(default)]
    attack: Option<PolySignal>,
    /// release time in seconds (default 0.1)
    #[deserr(default)]
    release: Option<PolySignal>,
    /// makeup gain as a dB-domain voltage (-5V = -24dB, 0V = unity, +5V = +24dB,
    /// default 0 = unity) — matches the inputGain / outputGain convention.
    #[deserr(default)]
    makeup: Option<PolySignal>,
    /// input gain control (-5V = -24dB, 0V = unity, 5V = +24dB) — drives signal into the compressor
    #[deserr(default)]
    input_gain: Option<PolySignal>,
    /// output gain control (-5V = -24dB, 0V = unity, 5V = +24dB) — trims level after compression
    #[deserr(default)]
    output_gain: Option<PolySignal>,
    /// dry/wet blend (0 = fully dry, 5 = fully wet, default 5.0)
    #[deserr(default)]
    mix: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CompressorOutputs {
    #[output("sample", "compressed signal", default)]
    sample: PolyOutput,
}

/// EXPERIMENTAL
///
/// Single-band feed-forward dynamics processor with peak envelope follower.
///
/// Two stages run in parallel:
///
/// - **downward stage** acts on signal above `threshold`. `ratio > 1` =
///   compression, `ratio < 1` = upward expansion (boost loud), `ratio = 1` =
///   passthrough.
/// - **upward stage** acts on signal below `upwardThreshold`. `upwardRatio > 1`
///   = upward compression (boost quiet), `upwardRatio < 1` = downward
///   expansion / gate-like attenuation of quiet signal, `upwardRatio = 1` =
///   passthrough.
///
/// Both ratios use textbook `X:1` notation (X = Δinput dB / Δoutput dB) and
/// are lower-bounded at 1e-3 (≈ Ableton's 1:1000 display). The detected
/// level is clamped to ±60 dB so expansion stays bounded at signal extremes.
///
/// An optional **side-chain** input lets the envelope follower track an
/// external signal while the gain is applied to `input` — useful for
/// ducking, kick-triggered pumping, or building multiband effects like OTT.
///
/// **Signal flow:** input → input gain → (detector: side-chain or input)
///   → up + down compression → output gain → dry/wet mix → output
///
/// - **threshold** — downward stage threshold in volts (0–5, default 2.5).
/// - **ratio** — downward stage ratio in textbook X:1 form. > 1 compresses,
///   < 1 expands (boosts loud signal), 1 = passthrough. Lower-bounded at
///   1e-3 (≈ 1:1000 expansion); upper bound is `f32::INFINITY` =
///   brick-wall limiter. Default 4.0 (= Ableton display 1:0.25).
/// - **upwardThreshold** — upward stage threshold in volts (0–5, default 0).
/// - **upwardRatio** — upward stage ratio in textbook X:1 form. > 1 boosts
///   quiet signal, < 1 gates quiet signal, 1 = passthrough. Same bounds as
///   `ratio`. Default 1 (passthrough).
/// - **sidechain** — optional external detector signal.
/// - **attack** / **release** — envelope follower time constants in seconds.
/// - **makeup** — post-compression makeup gain as a dB-domain voltage
///   (-5V = -24dB, 0V = unity, +5V = +24dB, default 0).
/// - **inputGain** — gain before the compressor (-5V = -24dB, 0V = unity,
///   5V = +24dB). Raising input gain drives more signal into the compressor.
/// - **outputGain** — gain after compression (-5V = -24dB, 0V = unity,
///   5V = +24dB). Trims the final output level.
/// - **mix** — dry/wet blend (0 = fully dry, 5 = fully wet, default 5.0).
///   The dry signal is the original input before any gain staging.
///
/// ```js
/// // simple bus compressor
/// $comp(input, { threshold: 2.5, ratio: 4, attack: 0.01, release: 0.1 })
/// ```
///
/// ```js
/// // side-chain ducking — kick triggers gain reduction on pad
/// $comp(pad, {
///   sidechain: kick,
///   threshold: 1.0, ratio: 8, attack: 0.005, release: 0.2,
/// })
/// ```
///
/// ```js
/// // multiband compression using $xover + $comp
/// let bands = $xover(input, { lowMidFreq: '200hz', midHighFreq: '2000hz' })
/// let low  = $comp(bands.low,  { threshold: 2.5, ratio: 4 })
/// let mid  = $comp(bands.mid,  { threshold: 3,   ratio: 3 })
/// let high = $comp(bands.high, { threshold: 2,   ratio: 6 })
/// $mix(low, mid, high).out()
/// ```
#[module(name = "$comp", args(input))]
pub struct Compressor {
    outputs: CompressorOutputs,
    state: CompressorState,
    params: CompressorParams,
}

impl Compressor {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();

        for ch in 0..channels {
            let state = &mut self.state.channels[ch];

            let input = self.params.input.get_value(ch);

            // Apply input gain
            let input_gain_voltage = self.params.input_gain.value_or(ch, 0.0);
            let gained = input * voltage_to_gain(input_gain_voltage);

            // Side-chain: detector reads from sidechain input if connected,
            // otherwise from the gain-staged input itself.
            let detector = match &self.params.sidechain {
                Some(sc) => sc.get_value(ch),
                None => gained,
            };

            // Read compressor parameters
            let threshold = self.params.threshold.value_or(ch, 2.5);
            let ratio = self.params.ratio.value_or(ch, 4.0);
            let upward_threshold = self.params.upward_threshold.value_or(ch, 0.0);
            let upward_ratio = self.params.upward_ratio.value_or(ch, 1.0);
            let attack = self.params.attack.value_or(ch, 0.01);
            let release = self.params.release.value_or(ch, 0.1);
            // makeup is a dB-domain voltage matching inputGain / outputGain
            // (-5 V = -24 dB, 0 V = unity, +5 V = +24 dB). voltage_to_gain
            // clamps the voltage internally.
            let makeup_voltage = self.params.makeup.value_or(ch, 0.0);
            let makeup = voltage_to_gain(makeup_voltage);

            // Compress
            let compressed = compress(
                gained,
                detector,
                &mut state.envelope,
                threshold,
                ratio,
                upward_threshold,
                upward_ratio,
                attack,
                release,
                makeup,
                sample_rate,
            );

            // Apply output gain
            let output_gain_voltage = self.params.output_gain.value_or(ch, 0.0);
            let out = compressed * voltage_to_gain(output_gain_voltage);

            // Dry/wet mix (dry signal is original input before gain staging)
            let mix_amount = self.params.mix.value_or(ch, 5.0).clamp(0.0, 5.0) / 5.0;
            let output = input * (1.0 - mix_amount) + out * mix_amount;

            self.outputs.sample.set(ch, output);
        }
    }
}

message_handlers!(impl Compressor {});

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive `compress()` to steady state on a constant DC level so the
    /// envelope follower fully tracks the input, then assert the output gain.
    fn steady_state_output(
        sample: f32,
        threshold: f32,
        ratio: f32,
        upward_threshold: f32,
        upward_ratio: f32,
    ) -> f32 {
        let mut env = sample.abs();
        let mut last = 0.0;
        // Long enough run to settle the one-pole follower at any sane attack.
        for _ in 0..2_000 {
            last = compress(
                sample,
                sample,
                &mut env,
                threshold,
                ratio,
                upward_threshold,
                upward_ratio,
                0.001,
                0.001,
                1.0,
                48_000.0,
            );
        }
        last
    }

    #[test]
    fn ratio_one_is_passthrough() {
        let out = steady_state_output(2.0, 1.0, 1.0, 0.0, 1.0);
        assert!((out - 2.0).abs() < 1e-3, "got {}", out);
    }

    #[test]
    fn standard_4to1_compression() {
        // Threshold 1V (= 0 dBV). Input 2V (= ~6 dBV) → 6 dB above. At 4:1 →
        // 1.5 dB above threshold = 10^(1.5/20) ≈ 1.189 V.
        let out = steady_state_output(2.0, 1.0, 4.0, 0.0, 1.0);
        let expected = 10f32.powf(1.5 / 20.0);
        assert!(
            (out - expected).abs() < 0.01,
            "got {}, expected {}",
            out,
            expected
        );
    }

    #[test]
    fn infinite_ratio_acts_as_brick_wall_limiter() {
        // At ratio = INFINITY, anything above threshold should be pinned to
        // the threshold level.
        let out = steady_state_output(5.0, 1.0, f32::INFINITY, 0.0, 1.0);
        assert!((out - 1.0).abs() < 0.01, "got {}", out);
    }

    #[test]
    fn ratio_below_one_expands_above_threshold() {
        // ratio = 0.5 (= 1:2 expansion): input 2V (= 6 dB above 1V threshold)
        // should be boosted by another 6 dB → ~4V output.
        let out = steady_state_output(2.0, 1.0, 0.5, 0.0, 1.0);
        assert!(out > 3.5 && out < 4.5, "got {}", out);
    }

    #[test]
    fn upward_compression_boosts_quiet() {
        // Quiet signal (0.1V = -20 dBV), upward threshold 1V (0 dBV), 4:1
        // upward → 0.1V should be boosted toward 1V.
        let quiet = steady_state_output(0.1, 5.0, 1.0, 1.0, 4.0);
        assert!(quiet > 0.1, "expected boost above 0.1, got {}", quiet);
        assert!(quiet < 1.0, "should not exceed threshold, got {}", quiet);
    }

    #[test]
    fn extreme_inputs_stay_finite() {
        // Sanity: pathological combinations shouldn't NaN or blow up to inf.
        for sample in [0.0_f32, 1e-10, 0.5, 5.0, 100.0] {
            for ratio in [1e-3_f32, 0.5, 1.0, 4.0, 1e6, f32::INFINITY] {
                let out = steady_state_output(sample, 1.0, ratio, 0.0, 1.0);
                assert!(
                    out.is_finite(),
                    "non-finite output for sample={} ratio={}: {}",
                    sample,
                    ratio,
                    out
                );
            }
        }
    }
}
