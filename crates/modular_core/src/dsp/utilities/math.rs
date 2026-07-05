use crate::dsp::utils::{hz_to_voct_f64, voct_to_hz_f64};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use crate::types::{ClockMessages, Connect};
use deserr::{DeserializeError, Deserr, ErrorKind, IntoValue, ValuePointerRef};
use fasteval::{Compiler, Evaler, Instruction};
use napi::Result;
use schemars::JsonSchema;
use std::sync::Arc;

/// Compiled fasteval expression data. Wrapped in `Arc` so that
/// `MathExpressionParam` can derive `Clone` cheaply (Arc clone)
/// without requiring `Slab`/`Instruction` to implement `Clone`.
struct MathCompiled {
    slab: fasteval::Slab,
    instruction: Instruction,
}

impl Default for MathCompiled {
    fn default() -> Self {
        Self {
            slab: fasteval::Slab::new(),
            instruction: Instruction::default(),
        }
    }
}

#[derive(Clone, Default, JsonSchema, Connect)]
#[serde(transparent)]
#[schemars(transparent)]
struct MathExpressionParam {
    #[allow(dead_code)]
    source: String,

    #[serde(skip)]
    #[schemars(skip)]
    compiled: Arc<MathCompiled>,
}

impl Connect for Arc<MathCompiled> {
    fn apply_default_connections(&mut self) {}
    fn connect(&mut self, _patch: &crate::Patch) {}
    fn collect_cables(&self, _sink: &mut Vec<String>) {}
    fn inject_index_ptr(&mut self, _ptr: *const std::cell::Cell<usize>) {}
}

impl MathExpressionParam {
    /// Parse a math expression string into a MathExpressionParam.
    fn parse(source: String) -> std::result::Result<Self, String> {
        let mut slab = fasteval::Slab::new();
        let parser = fasteval::Parser::new();
        let instruction = match parser.parse(&source, &mut slab.ps) {
            Err(e) => {
                return Err(format!("Failed to parse expression: {}", e));
            }
            Ok(expression) => expression.from(&slab.ps).compile(&slab.ps, &mut slab.cs),
        };

        Ok(MathExpressionParam {
            source,
            compiled: Arc::new(MathCompiled { slab, instruction }),
        })
    }
}

// deserr implementation for MathExpressionParam - transparent string wrapper that parses.
impl<E: DeserializeError> deserr::Deserr<E> for MathExpressionParam {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> std::result::Result<Self, E> {
        let source = String::deserialize_from_value(value, location)?;
        Self::parse(source).map_err(|e| {
            deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected { msg: e },
                location,
            ))
        })
    }
}

#[derive(Clone, Deserr, JsonSchema, ChannelCount, SignalParams, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct MathParams {
    /// math expression to evaluate (e.g. "x * 2 + sin(t)")
    expression: MathExpressionParam,
    /// first input variable, referenced as `x` in the expression
    #[deserr(default)]
    x: Option<PolySignal>,
    /// second input variable, referenced as `y` in the expression
    #[deserr(default)]
    y: Option<PolySignal>,
    /// third input variable, referenced as `z` in the expression
    #[deserr(default)]
    z: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MathOutputs {
    #[output("output", "result of the expression", default)]
    output: PolyOutput,
}

/// State for the Math module.
struct MathState {
    phase: f32,
    loop_index: usize,
    running: bool,
}

impl Default for MathState {
    fn default() -> Self {
        Self {
            phase: 0.0,
            loop_index: 0,
            running: true,
        }
    }
}

/// Evaluates a math expression every sample, giving you arbitrary control
/// voltage transformations.
///
/// Write an expression string using `x`, `y`, `z` as input variables.
/// The built-in variable `t` (time in seconds) is also available.
///
/// The inputs are polyphonic: the module is as wide as its widest input, the
/// expression is evaluated once per channel, and narrower inputs cycle. A
/// non-finite result (e.g. `0/0`) is emitted as 0.
///
/// **Functions:** `sin`, `cos`, `tan`, `asin`, `acos`, `atan`,
/// `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh`,
/// `log(base?, val)`, `abs`, `sign`, `int`, `ceil`, `floor`,
/// `round(modulus?, val)`, `min(val, ...)`, `max(val, ...)`,
/// `e()`, `pi()`, `vToHz(volts)`, `hzToV(hz)`
///
/// **Operators** (highest to lowest precedence):
/// `^`, `%`, `/`, `*`, `-`, `+`,
/// `== != < <= >= >`,
/// `&& and`, `|| or`
///
/// ```js
/// // crossfade between two oscillators
/// $math("x * sin(t) + y * cos(t)", { x: $saw('c3'), y: $pulse('c3') })
/// ```
#[module(name = "$math", args(expression))]
pub struct Math {
    outputs: MathOutputs,
    params: MathParams,
    state: MathState,
}

message_handlers!(impl Math {
    Clock(m) => Math::on_clock_message,
});

impl Math {
    fn update(&mut self, sample_rate: f32) {
        // Update time
        if self.state.running {
            self.state.phase += 1.0 / sample_rate;
            if self.state.phase >= 1.0 {
                self.state.phase -= 1.0;
                self.state.loop_index += 1;
            }
        }

        for ch in 0..self.channel_count() {
            let value = self.eval(ch).unwrap_or(0.0) as f32;
            // PolyOutput::set sanitizes non-finite results, so an expression
            // like 0/0 puts 0.0 on the cable rather than NaN.
            self.outputs.output.set(ch, value);
        }
    }

    fn eval(&mut self, ch: usize) -> std::result::Result<f64, fasteval::Error> {
        let x = self.params.x.value_or_zero(ch) as f64;
        let y = self.params.y.value_or_zero(ch) as f64;
        let z = self.params.z.value_or_zero(ch) as f64;
        let t = self.state.phase as f64 + self.state.loop_index as f64;

        let mut cb = |name: &str, args: Vec<f64>| -> Option<f64> {
            match name {
                "x" => Some(x),
                "y" => Some(y),
                "z" => Some(z),
                "t" => Some(t),
                "vToHz" => args.first().map(|v| voct_to_hz_f64(*v)),
                "hzToV" => args.first().map(|v| hz_to_voct_f64(*v)),
                // A wildcard to handle all undefined names:
                _ => None,
            }
        };

        Ok({
            let evaler = &self.params.expression.compiled.instruction;
            if let fasteval::IConst(c) = evaler {
                *c
            } else {
                evaler.eval(&self.params.expression.compiled.slab, &mut cb)?
            }
        })
    }

    fn on_clock_message(&mut self, m: &ClockMessages) -> Result<()> {
        match m {
            ClockMessages::Start => {
                self.state.running = true;
                self.state.phase = 0.0;
                self.state.loop_index = 0;
            }
            ClockMessages::Stop => {
                self.state.running = false;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48_000.0;

    fn make(expression: &str, channels: usize, params: [Option<PolySignal>; 3]) -> Math {
        let [x, y, z] = params;
        let mut outputs = MathOutputs::default();
        outputs.set_all_channels(channels);
        Math {
            outputs,
            params: MathParams {
                expression: MathExpressionParam::parse(expression.to_string()).unwrap(),
                x,
                y,
                z,
            },
            state: MathState::default(),
            _channel_count: channels,
            _block_index: Default::default(),
        }
    }

    fn poly(values: &[f32]) -> Option<PolySignal> {
        let signals: Vec<Signal> = values.iter().map(|&v| Signal::Volts(v)).collect();
        Some(PolySignal::poly(&signals))
    }

    #[test]
    fn evaluates_expression_per_channel() {
        // The module is as wide as its widest input; narrower inputs cycle.
        let mut m = make("x * 2 + y", 2, [poly(&[1.0, 3.0]), poly(&[10.0]), None]);
        m.update(SR);
        assert_eq!(m.outputs.output.get(0), 12.0);
        assert_eq!(m.outputs.output.get(1), 16.0);
    }

    #[test]
    fn non_finite_result_emits_zero() {
        // Division by zero (and 0/0) must land on the cable as 0.0, never as
        // inf/NaN that could lodge in downstream recursive state.
        let mut m = make("x / y", 2, [poly(&[1.0, 0.0]), poly(&[0.0]), None]);
        m.update(SR);
        assert_eq!(m.outputs.output.get(0), 0.0); // 1/0 → inf → 0
        assert_eq!(m.outputs.output.get(1), 0.0); // 0/0 → NaN → 0
    }
}
