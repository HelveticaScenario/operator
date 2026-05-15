use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::wrap,
    poly::{PolyOutput, PolySignal, PolySignalExt},
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct WrapParams {
    /// signal to wrap
    input: PolySignal,
    /// lower bound of the wrap range
    #[signal(default = 0.0)]
    min: PolySignal,
    /// upper bound of the wrap range
    #[signal(default = 5.0)]
    max: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct WrapOutputs {
    #[output("output", "wrapped signal output", default)]
    sample: PolyOutput,
}

/// Folds a signal into a range by wrapping values that exceed the boundaries
/// back from the opposite side — like a phase accumulator.
///
/// Both **min** and **max** accept polyphonic signals. If **max** < **min**
/// the bounds are swapped automatically.
///
/// ```js
/// // wrap a ramp into 0–5 V
/// $wrap($sine('1hz'), 0, 5).out()
/// ```
#[module(name = "$wrap", args(input, min, max))]
pub struct Wrap {
    outputs: WrapOutputs,
    params: WrapParams,
}

impl Wrap {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();

        for i in 0..channels as usize {
            let val = self.params.input.get_value(i);
            let a = self.params.min.get_value(i);
            let b = self.params.max.get_value(i);
            let (min, max) = if b < a { (b, a) } else { (a, b) };

            let output = if (max - min).abs() < f32::EPSILON {
                min
            } else {
                wrap(min..max, val)
            };

            self.outputs.sample.set(i, output);
        }
    }
}

message_handlers!(impl Wrap {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{poly::PolySignal, types::Signal};

    fn run_wrap(input: f32, min: f32, max: f32) -> f32 {
        let mut module = Wrap {
            outputs: WrapOutputs::default(),
            params: WrapParams {
                input: PolySignal::mono(Signal::Volts(input)),
                min: PolySignal::mono(Signal::Volts(min)),
                max: PolySignal::mono(Signal::Volts(max)),
            },
            _channel_count: 1,
        };
        module.outputs.sample.set_channels(1);
        module.update(44100.0);
        module.outputs.sample.get(0)
    }

    #[test]
    fn wrap_within_range_unchanged() {
        let result = run_wrap(2.5, 0.0, 5.0);
        assert!((result - 2.5).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_above_max_folds_back() {
        // 6.0 in [0, 5) → 1.0
        let result = run_wrap(6.0, 0.0, 5.0);
        assert!((result - 1.0).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_below_min_folds_forward() {
        // -1.0 in [0, 5) → 4.0
        let result = run_wrap(-1.0, 0.0, 5.0);
        assert!((result - 4.0).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_exactly_at_max_wraps_to_min() {
        // 5.0 in [0, 5) → 0.0 (exclusive upper bound)
        let result = run_wrap(5.0, 0.0, 5.0);
        assert!((result - 0.0).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_exactly_at_min_stays() {
        let result = run_wrap(0.0, 0.0, 5.0);
        assert!((result - 0.0).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_swaps_when_max_less_than_min() {
        // max=0, min=5 → treated as [0, 5); 6 → 1
        let result = run_wrap(6.0, 5.0, 0.0);
        assert!((result - 1.0).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_degenerate_zero_width_outputs_min() {
        let result = run_wrap(3.0, 2.0, 2.0);
        assert!((result - 2.0).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_negative_range() {
        // 0.5 in [-1, 1) → 0.5
        let result = run_wrap(0.5, -1.0, 1.0);
        assert!((result - 0.5).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn wrap_far_above_range_multiple_cycles() {
        // 11.0 in [0, 5) → 1.0 (two full cycles above)
        let result = run_wrap(11.0, 0.0, 5.0);
        assert!((result - 1.0).abs() < 1e-5, "got {result}");
    }
}
