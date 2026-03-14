use schemars::JsonSchema;
use serde::Deserialize;

use crate::poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS};
use crate::types::PolySignalFields;

fn default_count() -> usize {
    1
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct UnisonParams {
    /// input signal to expand (typically V/Oct pitch)
    input: PolySignal,
    /// number of unison voices per input channel (1–16)
    #[serde(default = "default_count")]
    count: usize,
    /// detune spread amount (0–10V, exponential: 0V = none, 10V = 1 octave)
    spread: Option<PolySignal>,
}

/// Custom channel count: max(all PolySignal channels) * count, clamped to 16.
#[allow(private_interfaces)]
pub fn unison_derive_channel_count(params: &UnisonParams) -> usize {
    let fields = params.poly_signal_fields();
    let refs: Vec<&PolySignal> = fields.into_iter().collect();
    let max_poly = PolySignal::max_channels(&refs).max(1);
    let count = params.count.clamp(1, PORT_MAX_CHANNELS);
    (max_poly * count).min(PORT_MAX_CHANNELS)
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UnisonOutputs {
    /// expanded polyphonic output with detuned copies
    #[output("output", "signal output with unison-expanded channels", default)]
    sample: PolyOutput,
}

/// Expands each input channel into multiple detuned copies for unison effects.
///
/// Takes a signal (typically V/Oct pitch) and multiplies channels by the
/// unison count, applying symmetric detuning controlled by the spread parameter.
///
/// - **count** — number of detuned copies per input channel (1–16)
/// - **spread** — detune amount with exponential curve (0–10V → 0–1 octave V/Oct)
///
/// Output channels = `input_channels × count`, clamped to 16.
///
/// ## Example
///
/// ```js
/// // 7-voice unison saw with moderate spread
/// $saw($unison('c4', 7, 5)).out()
///
/// // With modulated spread
/// $saw($unison($midiCV().pitch, 5, $sine('0.2hz'))).out()
/// ```
#[module(name = "$unison", channels_derive = unison_derive_channel_count, args(input, count, spread))]
pub struct Unison {
    outputs: UnisonOutputs,
    params: UnisonParams,
}

impl Unison {
    fn update(&mut self, _sample_rate: f32) {
        let count = self.params.count.clamp(1, PORT_MAX_CHANNELS);
        let input = &self.params.input;
        let spread = &self.params.spread;

        // Determine input channel count (at least 1)
        let input_channels = input.channels().max(1);

        let output_channels = self.channel_count();

        for input_ch in 0..input_channels {
            let input_val = input.get_value(input_ch);
            // Spread cycles across input channels
            let spread_v = spread.value_or(input_ch, 0.0).clamp(0.0, 10.0);
            // Exponential mapping: (spread_v / 10)^2 gives 0–1 V/Oct (0–1 octave)
            let normalized = spread_v / 10.0;
            let max_detune_voct = normalized * normalized;

            for voice in 0..count {
                let out_ch = input_ch * count + voice;
                if out_ch >= output_channels {
                    return;
                }

                let offset = if count > 1 {
                    // Symmetric fan-out: -max_detune to +max_detune
                    max_detune_voct * (2.0 * voice as f32 / (count - 1) as f32 - 1.0)
                } else {
                    0.0
                };

                self.outputs.sample.set(out_ch, input_val + offset);
            }
        }
    }
}

message_handlers!(impl Unison {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    /// Create a Unison with params and properly initialize _channel_count and output channels.
    fn make_unison(params: UnisonParams) -> Unison {
        let channels = unison_derive_channel_count(&params);
        let mut outputs = UnisonOutputs::default();
        outputs.set_all_channels(channels);
        Unison {
            params,
            outputs,
            _channel_count: channels,
        }
    }

    #[test]
    fn test_passthrough_count_1() {
        let mut u = make_unison(UnisonParams {
            input: PolySignal::mono(Signal::Volts(1.0)),
            count: 1,
            spread: Some(PolySignal::mono(Signal::Volts(5.0))),
        });
        u.update(48000.0);
        assert_eq!(u.outputs.sample.channels(), 1);
        assert_eq!(u.outputs.sample.get(0), 1.0);
    }

    #[test]
    fn test_no_spread_duplicates() {
        // count=3, spread=0 -> 3 identical copies
        let mut u = make_unison(UnisonParams {
            input: PolySignal::mono(Signal::Volts(2.0)),
            count: 3,
            spread: None,
        });
        u.update(48000.0);
        assert_eq!(u.outputs.sample.channels(), 3);
        assert_eq!(u.outputs.sample.get(0), 2.0);
        assert_eq!(u.outputs.sample.get(1), 2.0);
        assert_eq!(u.outputs.sample.get(2), 2.0);
    }

    #[test]
    fn test_symmetric_spread() {
        // count=3, spread=10V -> max_detune = 1.0 V/Oct
        // voice 0: -1.0, voice 1: 0.0, voice 2: +1.0
        let mut u = make_unison(UnisonParams {
            input: PolySignal::mono(Signal::Volts(0.0)),
            count: 3,
            spread: Some(PolySignal::mono(Signal::Volts(10.0))),
        });
        u.update(48000.0);
        assert_eq!(u.outputs.sample.channels(), 3);
        assert!((u.outputs.sample.get(0) - (-1.0)).abs() < 1e-6);
        assert!((u.outputs.sample.get(1) - 0.0).abs() < 1e-6);
        assert!((u.outputs.sample.get(2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_exponential_curve() {
        // spread=5V -> (5/10)^2 = 0.25 V/Oct
        let mut u = make_unison(UnisonParams {
            input: PolySignal::mono(Signal::Volts(0.0)),
            count: 3,
            spread: Some(PolySignal::mono(Signal::Volts(5.0))),
        });
        u.update(48000.0);
        assert!((u.outputs.sample.get(0) - (-0.25)).abs() < 1e-6);
        assert!((u.outputs.sample.get(1) - 0.0).abs() < 1e-6);
        assert!((u.outputs.sample.get(2) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn test_poly_input_expansion() {
        // 2-channel input, count=3 -> 6 output channels
        let mut u = make_unison(UnisonParams {
            input: PolySignal::poly(&[Signal::Volts(0.0), Signal::Volts(1.0)]),
            count: 3,
            spread: Some(PolySignal::mono(Signal::Volts(10.0))),
        });
        u.update(48000.0);
        assert_eq!(u.outputs.sample.channels(), 6);
        // Input ch 0 (0.0V): voices at -1.0, 0.0, +1.0
        assert!((u.outputs.sample.get(0) - (-1.0)).abs() < 1e-6);
        assert!((u.outputs.sample.get(1) - 0.0).abs() < 1e-6);
        assert!((u.outputs.sample.get(2) - 1.0).abs() < 1e-6);
        // Input ch 1 (1.0V): voices at 0.0, 1.0, 2.0
        assert!((u.outputs.sample.get(3) - 0.0).abs() < 1e-6);
        assert!((u.outputs.sample.get(4) - 1.0).abs() < 1e-6);
        assert!((u.outputs.sample.get(5) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_clamp_to_16_channels() {
        // 4-channel input, count=5 -> 20 desired, clamped to 16
        let mut u = make_unison(UnisonParams {
            input: PolySignal::poly(&[
                Signal::Volts(0.0),
                Signal::Volts(1.0),
                Signal::Volts(2.0),
                Signal::Volts(3.0),
            ]),
            count: 5,
            spread: None,
        });
        u.update(48000.0);
        assert_eq!(u.outputs.sample.channels(), 16);
    }

    #[test]
    fn test_channel_count_derivation() {
        // 1 channel * 7 = 7
        let params = UnisonParams {
            input: PolySignal::mono(Signal::Volts(0.0)),
            count: 7,
            spread: None,
        };
        assert_eq!(unison_derive_channel_count(&params), 7);

        // 3 channels * 5 = 15
        let params = UnisonParams {
            input: PolySignal::poly(&[Signal::Volts(0.0), Signal::Volts(0.0), Signal::Volts(0.0)]),
            count: 5,
            spread: None,
        };
        assert_eq!(unison_derive_channel_count(&params), 15);

        // 3 channels * 6 = 18, clamped to 16
        let params = UnisonParams {
            input: PolySignal::poly(&[Signal::Volts(0.0), Signal::Volts(0.0), Signal::Volts(0.0)]),
            count: 6,
            spread: None,
        };
        assert_eq!(unison_derive_channel_count(&params), 16);
    }

    #[test]
    fn test_spread_cycles_across_input_channels() {
        // 2-channel input, 2-channel spread with different values
        let mut u = make_unison(UnisonParams {
            input: PolySignal::poly(&[Signal::Volts(0.0), Signal::Volts(0.0)]),
            count: 2,
            spread: Some(PolySignal::poly(&[
                Signal::Volts(10.0), // ch 0: 1.0 V/Oct detune
                Signal::Volts(0.0),  // ch 1: 0.0 V/Oct detune
            ])),
        });
        u.update(48000.0);
        assert_eq!(u.outputs.sample.channels(), 4);
        // Input ch 0 with spread 10V: voices at -1.0, +1.0
        assert!((u.outputs.sample.get(0) - (-1.0)).abs() < 1e-6);
        assert!((u.outputs.sample.get(1) - 1.0).abs() < 1e-6);
        // Input ch 1 with spread 0V: voices at 0.0, 0.0
        assert!((u.outputs.sample.get(2) - 0.0).abs() < 1e-6);
        assert!((u.outputs.sample.get(3) - 0.0).abs() < 1e-6);
    }
}
