use crate::dsp::utils::{hz_to_voct_f64, voct_to_hz_f64};
use crate::poly::{MonoSignal, MonoSignalExt};
use crate::types::{ClockMessages, Connect};
use deserr::{DeserializeError, Deserr, ErrorKind, IntoValue, ValuePointerRef};
use fasteval::{Compiler, Evaler, Instruction};
use napi::Result;
use schemars::JsonSchema;
use std::collections::BTreeMap;
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

#[derive(Clone, Default, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
struct MathExpressionParam {
    #[allow(dead_code)]
    source: String,

    #[serde(skip)]
    #[schemars(skip)]
    compiled: Arc<MathCompiled>,
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

impl Connect for MathExpressionParam {
    fn connect(&mut self, _patch: &crate::Patch) {}
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
    x: Option<MonoSignal>,
    /// second input variable, referenced as `y` in the expression
    #[deserr(default)]
    y: Option<MonoSignal>,
    /// third input variable, referenced as `z` in the expression
    #[deserr(default)]
    z: Option<MonoSignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MathOutputs {
    #[output("output", "result of the expression", default)]
    output: f32,
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

        self.outputs.output = self.eval().unwrap_or(0.0) as f32;
    }

    fn eval(&mut self) -> std::result::Result<f64, fasteval::Error> {
        let x = self.params.x.value_or(0.0) as f64;
        let y = self.params.y.value_or(0.0) as f64;
        let z = self.params.z.value_or(0.0) as f64;
        let t = self.state.phase as f64 + self.state.loop_index as f64;

        let mut btree = BTreeMap::new();
        btree.insert("x".to_string(), x);
        btree.insert("y".to_string(), y);
        btree.insert("z".to_string(), z);
        btree.insert("t".to_string(), t);

        let mut cb = move |name: &str, args: Vec<f64>| -> Option<f64> {
            if let Some(val) = btree.get(name) {
                return Some(*val);
            }
            match name {
                "vToHz" => args.first().map(|v| voct_to_hz_f64(*v)),
                "hzToV" => args.first().map(|v| hz_to_voct_f64(*v)),

                // A wildcard to handle all undefined names:
                _ => None,
            }
        };
        // let mut ns = fasteval::CachedCallbackNamespace::new(cb);

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
