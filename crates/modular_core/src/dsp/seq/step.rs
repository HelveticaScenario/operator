use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    MonoSignal,
    dsp::utils::SchmittTrigger,
    param_errors::ModuleParamErrors,
    poly::{PolyOutput, PolySignal},
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = validate_step_params -> ModuleParamErrors)]
struct StepParams {
    /// Steps of the sequence
    steps: Vec<PolySignal>,
    /// Next step trigger
    next: MonoSignal,
    /// Reset trigger
    #[deserr(default)]
    reset: Option<MonoSignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StepOutputs {
    #[output("output", "forwarded signal", default)]
    sample: PolyOutput,
}

fn validate_step_params(
    params: StepParams,
    _location: deserr::ValuePointerRef,
) -> Result<StepParams, ModuleParamErrors> {
    if params.steps.is_empty() {
        let mut err = ModuleParamErrors::default();
        err.add(
            "steps".to_string(),
            "must have at least one step".to_string(),
        );
        return Err(err);
    }
    Ok(params)
}

fn step_derive_channel_count(params: &StepParams) -> usize {
    params.steps.iter().map(|s| s.channels()).max().unwrap_or(0)
}

/// Step sequencer
#[module(name = "$step", channels_derive = step_derive_channel_count, args(steps, next))]
pub struct Step {
    outputs: StepOutputs,
    params: StepParams,
    state: StepState,
}

struct StepState {
    current_step_idx: usize,
    next_schmitt: SchmittTrigger,
    reset_schmitt: SchmittTrigger,
    first_update: bool,
}

impl Default for StepState {
    fn default() -> Self {
        Self {
            current_step_idx: 0,
            next_schmitt: SchmittTrigger::default(),
            reset_schmitt: SchmittTrigger::default(),
            first_update: true,
        }
    }
}
impl Step {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();

        if self.state.first_update {
            // Prime the Schmitt triggers but don't act on any edges they report.
            self.state
                .next_schmitt
                .process(self.params.next.get_value());
            if let Some(ref reset) = self.params.reset {
                self.state.reset_schmitt.process(reset.get_value());
            }
            self.state.first_update = false;
        } else {
            if self
                .state
                .next_schmitt
                .process(self.params.next.get_value())
            {
                self.state.current_step_idx += 1;
                if self.state.current_step_idx >= self.params.steps.len() {
                    self.state.current_step_idx = 0;
                }
            }

            if let Some(ref reset) = self.params.reset
                && self.state.reset_schmitt.process(reset.get_value())
            {
                self.state.current_step_idx = 0;
            }
        }

        let idx = self.state.current_step_idx.min(self.params.steps.len() - 1);
        let step = &self.params.steps[idx];
        for i in 0..channels as usize {
            self.outputs.sample.set(i, step.get_value(i));
        }
    }
}

message_handlers!(impl Step {});
