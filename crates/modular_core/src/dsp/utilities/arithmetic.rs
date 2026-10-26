use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input;
use crate::poly::{PolyOutput, PolySignal};

/// Operation `_arithmetic` applies between its input and operand.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "snake_case")]
#[deserr(rename_all = lowercase)]
pub enum ArithmeticOp {
    #[default]
    Add,
    Sub,
    Mul,
    Div,
    /// `input` raised to `operand`, mirrored for negative inputs.
    Pow,
}

/// `x` raised to `y`, with the sign of `x`, so a negative input mirrors its
/// positive counterpart.
#[inline(always)]
fn signed_pow(x: f32, y: f32) -> f32 {
    x.signum() * x.abs().powf(y)
}

impl ArithmeticOp {
    #[inline(always)]
    fn apply(self, x: f32, y: f32) -> f32 {
        match self {
            Self::Add => x + y,
            Self::Sub => x - y,
            Self::Mul => x * y,
            Self::Div => x / y,
            Self::Pow => signed_pow(x, y),
        }
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ArithmeticParams {
    /// left-hand side
    input: PolySignal,
    /// right-hand side
    operand: PolySignal,
    /// operation applied as `input op operand`
    #[serde(default)]
    #[deserr(default)]
    op: ArithmeticOp,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ArithmeticOutputs {
    #[output("output", "result", default)]
    sample: PolyOutput,
}

/// Adds, subtracts, multiplies, divides or raises one signal to the power of
/// another, sample by sample. Division by zero outputs 0 V.
///
/// Backs the DSL's `.add()`, `.sub()`, `.mul()`, `.div()` and `.exp()` signal
/// methods.
#[module(name = "_arithmetic", args(input, operand), block)]
pub struct Arithmetic {
    outputs: ArithmeticOutputs,
    params: ArithmeticParams,
}

impl Arithmetic {
    fn update(&mut self, _sample_rate: f32) {
        let op = self.params.op;
        for i in 0..self.channel_count() {
            let value = op.apply(
                self.params.input.get_value(i),
                self.params.operand.get_value(i),
            );
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut ArithmeticBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), self.channel_count(), &mut scratch)?;
        let out = &mut outputs.sample;
        let inputs = [inputs.input, inputs.operand];
        // One loop per operation, so each runs without a branch per sample.
        match self.params.op {
            ArithmeticOp::Add => block_input::map(out, range, inputs, |[x, y]| x + y),
            ArithmeticOp::Sub => block_input::map(out, range, inputs, |[x, y]| x - y),
            ArithmeticOp::Mul => block_input::map(out, range, inputs, |[x, y]| x * y),
            ArithmeticOp::Div => block_input::map(out, range, inputs, |[x, y]| x / y),
            ArithmeticOp::Pow => block_input::map(out, range, inputs, |[x, y]| signed_pow(x, y)),
        }
        Some(())
    }
}

message_handlers!(impl Arithmetic {});

/// How `_round` rounds its input to a whole number.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "snake_case")]
#[deserr(rename_all = lowercase)]
pub enum RoundMode {
    /// To the nearest whole number, halves away from zero.
    #[default]
    Round,
    /// Down, towards negative infinity.
    Floor,
    /// Up, towards positive infinity.
    Ceil,
}

impl RoundMode {
    #[inline(always)]
    fn apply(self, x: f32) -> f32 {
        match self {
            Self::Round => x.round(),
            Self::Floor => x.floor(),
            Self::Ceil => x.ceil(),
        }
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct RoundParams {
    /// signal to round
    input: PolySignal,
    /// rounding direction
    #[serde(default)]
    #[deserr(default)]
    mode: RoundMode,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RoundOutputs {
    #[output("output", "rounded signal", default)]
    sample: PolyOutput,
}

/// Rounds a signal to a whole number of volts.
///
/// Backs the DSL's `.round()`, `.floor()` and `.ceil()` signal methods.
#[module(name = "_round", args(input), block)]
pub struct Round {
    outputs: RoundOutputs,
    params: RoundParams,
}

impl Round {
    fn update(&mut self, _sample_rate: f32) {
        let mode = self.params.mode;
        for i in 0..self.channel_count() {
            let value = mode.apply(self.params.input.get_value(i));
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut RoundBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), self.channel_count(), &mut scratch)?;
        let out = &mut outputs.sample;
        let inputs = [inputs.input];
        match self.params.mode {
            RoundMode::Round => block_input::map(out, range, inputs, |[x]| x.round()),
            RoundMode::Floor => block_input::map(out, range, inputs, |[x]| x.floor()),
            RoundMode::Ceil => block_input::map(out, range, inputs, |[x]| x.ceil()),
        }
        Some(())
    }
}

message_handlers!(impl Round {});
