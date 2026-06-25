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
    /// when true, shifts by whole integers to preserve the fractional part (pitch class in V/Oct)
    #[serde(default)]
    #[deserr(default)]
    octave: bool,
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
/// Set **octave** to `true` to shift by whole integers instead, preserving the
/// fractional part (pitch class in V/Oct signals). Always exact when
/// `max − min ≥ 1`; makes the closest-octave attempt otherwise.
///
/// ```js
/// // wrap a saw into 0–5 V
/// $wrap($saw('c3'), 0, 5)
/// // keep pitch class, clamp to octave range 0–2 V
/// $wrap($cycle($p('c2 c5 c3')), 0, 2, { octave: true })
/// ```
#[module(name = "$wrap", args(input, min, max))]
pub struct Wrap {
    outputs: WrapOutputs,
    params: WrapParams,
}

// Closed [lo, hi] variant — same as wrap(lo..hi) except val == hi returns hi.
fn wrap_closed(val: f32, lo: f32, hi: f32) -> f32 {
    if val == hi { hi } else { wrap(lo..hi, val) }
}

fn wrap_octave(wrapped: f32, x: f32) -> f32 {
    let f = x - x.floor();
    let n = (wrapped - f).round() as i32;
    f + n as f32
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
            } else if self.params.octave {
                wrap_octave(wrap_closed(val, min, max), val)
            } else {
                wrap_closed(val, min, max)
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
                octave: false,
            },
            _channel_count: 1,
            _block_index: Default::default(),
        };
        module.outputs.sample.set_channels(1);
        module.update(44100.0);
        module.outputs.sample.get(0)
    }

    fn run_wrap_octave(input: f32, min: f32, max: f32) -> f32 {
        let mut module = Wrap {
            outputs: WrapOutputs::default(),
            params: WrapParams {
                input: PolySignal::mono(Signal::Volts(input)),
                min: PolySignal::mono(Signal::Volts(min)),
                max: PolySignal::mono(Signal::Volts(max)),
                octave: true,
            },
            _channel_count: 1,
            _block_index: Default::default(),
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
    fn wrap_exactly_at_max_stays() {
        // 5.0 in [0, 5] → 5.0 (inclusive upper bound)
        let result = run_wrap(5.0, 0.0, 5.0);
        assert!((result - 5.0).abs() < 1e-5, "got {result}");
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

    // octave mode

    #[test]
    fn octave_within_range_unchanged() {
        // 1.5 already in [0, 2) — no shift needed
        let result = run_wrap_octave(1.5, 0.0, 2.0);
        assert!((result - 1.5).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn octave_shifts_down_to_fit() {
        // 3.5 → pitch class 0.5, window [0, 2): closest valid octave is 1.5
        let result = run_wrap_octave(3.5, 0.0, 2.0);
        assert!((result - 1.5).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn octave_shifts_up_to_fit() {
        // -1.5 → pitch class 0.5, window [0, 2): shifts to 0.5
        let result = run_wrap_octave(-1.5, 0.0, 2.0);
        assert!((result - 0.5).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn octave_preserves_frac_across_many_octaves() {
        // 7.3 → pitch class ~0.3, window [0, 2): n_ideal=7 clamped to n_max=1, output ~1.3
        let result = run_wrap_octave(7.3, 0.0, 2.0);
        assert!((result - 1.3).abs() < 1e-4, "got {result}");
    }

    #[test]
    fn octave_negative_input_frac_positive() {
        // -0.3 wraps to 1.7 in [0, 2); frac of -0.3 is 0.7, frac of 1.7 is 0.7 — already matches
        let result = run_wrap_octave(-0.3, 0.0, 2.0);
        assert!((result - 1.7).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn octave_impossible_best_attempt_above() {
        // frac 0.9, window [0.2, 0.7): wraps to ~0.4; nearest same-frac to 0.4 is -0.1 or 0.9 (equidistant); f32 rounds to -0.1
        let result = run_wrap_octave(1.9, 0.2, 0.7);
        assert!((result - (-0.1)).abs() < 1e-5, "got {result}");
    }

    #[test]
    fn octave_impossible_best_attempt_below() {
        // frac 0.1, window [0.3, 0.8): wraps to ~0.6; nearest same-frac to 0.6 is 1.1 (dist 0.5) vs -0.9 (dist 1.5) → 1.1
        let result = run_wrap_octave(1.1, 0.3, 0.8);
        assert!((result - 1.1).abs() < 1e-4, "got {result}");
    }

    #[test]
    fn octave_span_exactly_one_always_fits() {
        // window [0.3, 1.3): span=1, frac 0.7 → 0.7; frac 0.1 → 0.1+1=1.1; frac 0.3 → 0.3
        assert!((run_wrap_octave(0.7, 0.3, 1.3) - 0.7).abs() < 1e-5);
        assert!((run_wrap_octave(2.1, 0.3, 1.3) - 1.1).abs() < 1e-4);
        assert!((run_wrap_octave(5.3, 0.3, 1.3) - 0.3).abs() < 1e-4);
    }
}
