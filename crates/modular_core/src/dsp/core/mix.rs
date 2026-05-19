use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
};

/// Mixing mode for combining input signals.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "snake_case")]
#[deserr(rename_all = lowercase)]
pub enum MixMode {
    /// Sum all inputs.
    #[default]
    Sum,
    /// Average all inputs.
    Average,
    /// Keep the strongest input.
    Max,
    /// Keep the weakest non-zero input.
    Min,
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
pub struct MixParams {
    /// Input signals to mix channel-by-channel.
    ///
    /// Channel `n` from every input is mixed into output channel `n`.
    pub inputs: Vec<PolySignal>,
    /// How inputs are combined.
    #[serde(default)]
    #[deserr(default)]
    mode: MixMode,
    /// Final output level (perceptual curve, exponent 3).
    #[signal(default = 5.0, range = (0.0, 10.0))]
    #[deserr(default)]
    pub gain: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MixOutputs {
    /// Mixed multichannel output.
    #[output(
        "output",
        "multichannel mix: each output channel mixes the same channel index from all inputs (not a mono fold-down)",
        default
    )]
    sample: PolyOutput,
}

/// Custom channel count derivation for Mix.
///
/// Mix output channels = max(max_input_channels, gain_channels), at least 1.
/// This matches the runtime behavior in update().
pub fn mix_derive_channel_count(params: &MixParams) -> usize {
    // Get max channel count from inputs
    let input_refs: Vec<&PolySignal> = params.inputs.iter().collect();

    let max_input_channels = if params.inputs.is_empty() {
        0usize
    } else {
        PolySignal::max_channels(&input_refs) as usize
    };

    // Get gain channel count
    let gain_channels = params.gain.channel_count();

    // Output channels = max(max_input_channels, gain_channels), at least 1 if inputs empty
    if params.inputs.is_empty() {
        gain_channels.max(1)
    } else {
        max_input_channels.max(gain_channels)
    }
    .min(PORT_MAX_CHANNELS)
}

/// Mix module for combining multiple signals into a single mix bus.
///
/// Use this when you want to blend several multichannel modulation/audio sources.
/// It mixes channel `n` across all inputs into output channel `n`, rather than
/// folding all channels into a single mono channel.
#[module(name = "$mix", channels_derive = mix_derive_channel_count, args(inputs))]
pub struct Mix {
    outputs: MixOutputs,
    params: MixParams,
    state: MixState,
}

/// State for the Mix module.
#[derive(Default)]
struct MixState {
    gain_buffer: [Clickless; PORT_MAX_CHANNELS],
}

message_handlers!(impl Mix {});

impl Mix {
    fn update(&mut self, _sample_rate: f32) {
        let inputs = &self.params.inputs;
        let gain = &self.params.gain;

        let output_channels = self.channel_count();

        // Empty inputs case — output silence.
        if inputs.is_empty() {
            for i in 0..output_channels {
                self.outputs.sample.set(i, 0.0);
            }
            return;
        }

        // Max channel count over all inputs.
        let mut max_input_channels: usize = 0;
        for input in inputs.iter() {
            let c = input.channels();
            if c > max_input_channels {
                max_input_channels = c;
            }
        }

        // Pre-compute mixed values for each input channel index.
        let mut pre_gain_values = [0.0f32; PORT_MAX_CHANNELS];
        for channel in 0..max_input_channels {
            let mut sum: f32 = 0.0;
            let mut contributor_count: usize = 0;
            // Max-by-abs accumulators.
            let mut max_abs: f32 = -1.0;
            let mut max_val: f32 = 0.0;
            // Min-by-abs accumulators (excludes zeros).
            let mut min_abs: f32 = f32::INFINITY;
            let mut min_val: f32 = 0.0;

            for input in inputs.iter() {
                if channel >= input.channels() {
                    continue;
                }
                let v = input.get_value(channel);
                sum += v;
                contributor_count += 1;
                let av = v.abs();
                // NaN comparisons return false, so NaN never replaces a
                // finite best. Matches old `partial_cmp().unwrap_or(Equal)`
                // semantics enough to satisfy the no-panic test.
                if av > max_abs {
                    max_abs = av;
                    max_val = v;
                }
                if v != 0.0 && av < min_abs {
                    min_abs = av;
                    min_val = v;
                }
            }

            pre_gain_values[channel] = match self.params.mode {
                MixMode::Sum => sum,
                MixMode::Average => {
                    if contributor_count > 0 {
                        sum / contributor_count as f32
                    } else {
                        0.0
                    }
                }
                MixMode::Max => {
                    if max_abs >= 0.0 {
                        max_val
                    } else {
                        0.0
                    }
                }
                MixMode::Min => {
                    if min_abs.is_finite() {
                        min_val
                    } else {
                        0.0
                    }
                }
            };
        }

        // Apply gain with cycling on pre_gain_values.
        for i in 0..output_channels {
            let pre_gain_index = i % max_input_channels;
            let pre_gain_value = pre_gain_values[pre_gain_index];
            let amp_val = gain.value_or(i, 5.0);
            let normalized = (amp_val.abs() / 5.0).max(0.0);
            let curved = amp_val.signum() * normalized.powf(3.0);
            self.state.gain_buffer[i].update(curved);
            let gain_value = *self.state.gain_buffer[i];
            self.outputs.sample.set(i, pre_gain_value * gain_value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};

    /// Create a Mix with params and properly initialize _channel_count and output channels.
    fn make_mix(params: MixParams) -> Mix {
        let channels = mix_derive_channel_count(&params);
        let mut outputs = MixOutputs::default();
        outputs.set_all_channels(channels);
        Mix {
            params,
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
            state: MixState::default(),
        }
    }

    #[test]
    fn test_mix_single_poly_sum() {
        let mut mixer = make_mix(MixParams {
            inputs: vec![PolySignal::poly(&[
                Signal::Volts(1.0),
                Signal::Volts(2.0),
                Signal::Volts(3.0),
            ])],
            mode: MixMode::Sum,
            gain: None,
        });
        mixer.update(48000.0);
        assert_eq!(mixer.outputs.sample.channels(), 3);
        assert_eq!(mixer.outputs.sample.get(0), 1.0);
        assert_eq!(mixer.outputs.sample.get(1), 2.0);
        assert_eq!(mixer.outputs.sample.get(2), 3.0);
    }

    #[test]
    fn test_mix_two_poly_sum() {
        // A: 2 channels [1, 2], B: 3 channels [10, 20, 30]
        let mut mixer = make_mix(MixParams {
            inputs: vec![
                PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(2.0)]),
                PolySignal::poly(&[
                    Signal::Volts(10.0),
                    Signal::Volts(20.0),
                    Signal::Volts(30.0),
                ]),
            ],
            mode: MixMode::Sum,
            gain: None,
        });
        mixer.update(48000.0);
        // Output should be 3 channels
        assert_eq!(mixer.outputs.sample.channels(), 3);
        // Channel 0: 1 + 10 = 11
        assert_eq!(mixer.outputs.sample.get(0), 11.0);
        // Channel 1: 2 + 20 = 22
        assert_eq!(mixer.outputs.sample.get(1), 22.0);
        // Channel 2: 0 + 30 = 30 (A has no channel 2, contributes 0)
        assert_eq!(mixer.outputs.sample.get(2), 30.0);
    }

    #[test]
    fn test_mix_average_mode() {
        // A: 2 channels [2, 4], B: 2 channels [6, 8]
        let mut mixer = make_mix(MixParams {
            inputs: vec![
                PolySignal::poly(&[Signal::Volts(2.0), Signal::Volts(4.0)]),
                PolySignal::poly(&[Signal::Volts(6.0), Signal::Volts(8.0)]),
            ],
            mode: MixMode::Average,
            gain: None,
        });
        mixer.update(48000.0);
        assert_eq!(mixer.outputs.sample.channels(), 2);
        // Channel 0: (2 + 6) / 2 = 4
        assert_eq!(mixer.outputs.sample.get(0), 4.0);
        // Channel 1: (4 + 8) / 2 = 6
        assert_eq!(mixer.outputs.sample.get(1), 6.0);
    }

    #[test]
    fn test_mix_gain_extends_channels() {
        // A: 1 channel [5], B: 2 channels [10, 20], gain: 3 channels [1, 2, 0.5]
        let mut mixer = make_mix(MixParams {
            inputs: vec![
                PolySignal::mono(Signal::Volts(5.0)),
                PolySignal::poly(&[Signal::Volts(10.0), Signal::Volts(20.0)]),
            ],
            mode: MixMode::Sum,
            gain: Some(PolySignal::poly(&[
                Signal::Volts(5.0),
                Signal::Volts(10.0),
                Signal::Volts(2.5),
            ])),
        });
        mixer.update(48000.0);
        // Output channels = max(2 input channels, 3 gain channels) = 3
        assert_eq!(mixer.outputs.sample.channels(), 3);
        // Channel 0: (5 + 10) * (5/5)^3 = 15 * 1.0 = 15
        assert_eq!(mixer.outputs.sample.get(0), 15.0);
        // Channel 1: (0 + 20) * (10/5)^3 = 20 * 8.0 = 160
        assert_eq!(mixer.outputs.sample.get(1), 160.0);
        // Channel 2: pre_gain cycles from channel 0 (15 pre-gain), gain[2] = (2.5/5)^3 = 0.125 -> 15 * 0.125 = 1.875
        assert_eq!(mixer.outputs.sample.get(2), 1.875);
    }

    #[test]
    fn test_mix_empty_inputs() {
        let mut mixer = make_mix(MixParams {
            inputs: vec![],
            mode: MixMode::Sum,
            gain: Some(PolySignal::poly(&[
                Signal::Volts(1.0),
                Signal::Volts(2.0),
                Signal::Volts(3.0),
            ])),
        });
        mixer.update(48000.0);
        // Empty inputs with 3-channel gain -> 3 channels of silence
        assert_eq!(mixer.outputs.sample.channels(), 3);
        assert_eq!(mixer.outputs.sample.get(0), 0.0);
        assert_eq!(mixer.outputs.sample.get(1), 0.0);
        assert_eq!(mixer.outputs.sample.get(2), 0.0);
    }

    #[test]
    fn test_mix_empty_inputs_no_gain() {
        let params: MixParams =
            deserr::deserialize::<MixParams, _, crate::param_errors::ModuleParamErrors>(
                serde_json::json!({"inputs": []}),
            )
            .unwrap();
        let mut mixer = make_mix(params);
        mixer.update(48000.0);
        // Empty inputs with no gain -> 1 channel of silence
        assert_eq!(mixer.outputs.sample.channels(), 1);
        assert_eq!(mixer.outputs.sample.get(0), 0.0);
    }

    #[test]
    fn test_mix_max_mode() {
        let mut mixer = make_mix(MixParams {
            inputs: vec![
                PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(-5.0)]),
                PolySignal::poly(&[Signal::Volts(-3.0), Signal::Volts(2.0)]),
            ],
            mode: MixMode::Max,
            gain: None,
        });
        mixer.update(48000.0);
        assert_eq!(mixer.outputs.sample.channels(), 2);
        // Channel 0: max by abs(1, -3) = -3
        assert_eq!(mixer.outputs.sample.get(0), -3.0);
        // Channel 1: max by abs(-5, 2) = -5
        assert_eq!(mixer.outputs.sample.get(1), -5.0);
    }

    #[test]
    fn test_mix_nan_input_does_not_panic() {
        for mode in [MixMode::Max, MixMode::Min] {
            let mut mixer = make_mix(MixParams {
                inputs: vec![
                    PolySignal::poly(&[Signal::Volts(f32::NAN)]),
                    PolySignal::poly(&[Signal::Volts(1.0)]),
                ],
                mode,
                gain: None,
            });
            mixer.update(48000.0);
            // Must not panic; output may be NaN or a finite value — either is acceptable
            let _ = mixer.outputs.sample.get(0);
        }
    }
}
