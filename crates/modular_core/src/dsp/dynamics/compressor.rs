//! Single-band feed-forward compressor module.
//!
//! Peak-detecting compressor with configurable threshold, ratio,
//! attack/release times, makeup gain, and input/output gain staging.

use schemars::JsonSchema;
use serde::Deserialize;

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
#[inline]
fn compress(
    sample: f32,
    envelope: &mut f32,
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    sample_rate: f32,
) -> f32 {
    let ratio = ratio.max(1.0);
    let threshold = threshold.max(0.0);
    let attack = attack.max(1e-6);
    let release = release.max(1e-6);

    // Envelope follower (peak detection with attack/release ballistics)
    let input_abs = sample.abs();
    let coeff = if input_abs > *envelope {
        (-1.0 / (attack * sample_rate)).exp()
    } else {
        (-1.0 / (release * sample_rate)).exp()
    };
    *envelope = input_abs + coeff * (*envelope - input_abs);

    // Gain computation in dB domain
    let level_db = 20.0 * (*envelope + 1e-10).log10();
    let threshold_db = 20.0 * (threshold + 1e-10).log10();

    let gain_db = if level_db > threshold_db {
        (threshold_db - level_db) * (1.0 - 1.0 / ratio)
    } else {
        0.0
    };

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

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct CompressorParams {
    /// audio input signal
    input: PolySignal,
    /// compression threshold (0-5V, default 2.5)
    threshold: Option<PolySignal>,
    /// compression ratio (1-20, default 4.0)
    ratio: Option<PolySignal>,
    /// attack time in seconds (default 0.01)
    attack: Option<PolySignal>,
    /// release time in seconds (default 0.1)
    release: Option<PolySignal>,
    /// makeup gain multiplier (0-5, default 1.0)
    makeup: Option<PolySignal>,
    /// input gain control (-5V = -24dB, 0V = unity, 5V = +24dB) — drives signal into the compressor
    input_gain: Option<PolySignal>,
    /// output gain control (-5V = -24dB, 0V = unity, 5V = +24dB) — trims level after compression
    output_gain: Option<PolySignal>,
    /// dry/wet blend (0 = fully dry, 5 = fully wet, default 5.0)
    mix: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CompressorOutputs {
    #[output("sample", "compressed signal", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// EXPERIMENTAL
///
/// Single-band feed-forward compressor with peak envelope follower.
///
/// Applies feed-forward compression in the log domain with configurable
/// threshold, ratio, attack/release ballistics, and makeup gain. Input and
/// output gain staging allow driving signal into the compressor and trimming
/// the output level independently. A dry/wet mix control enables parallel
/// compression.
///
/// **Signal flow:** input → input gain → compressor → output gain → dry/wet mix → output
///
/// - **threshold** — compression threshold in volts (0–5, default 2.5).
/// - **ratio** — compression ratio (1–20, default 4.0).
/// - **attack** / **release** — envelope follower time constants in seconds.
/// - **makeup** — post-compression makeup gain (linear multiplier, 0–5).
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

            // Read compressor parameters
            let threshold = self.params.threshold.value_or(ch, 2.5);
            let ratio = self.params.ratio.value_or(ch, 4.0);
            let attack = self.params.attack.value_or(ch, 0.01);
            let release = self.params.release.value_or(ch, 0.1);
            let makeup = self.params.makeup.value_or(ch, 1.0);

            // Compress
            let compressed = compress(
                gained,
                &mut state.envelope,
                threshold,
                ratio,
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
