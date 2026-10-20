//! Groove: a periodic, monotonic time map applied to a playhead.
//!
//! Grooves reach sequencers as an offset (grooved minus straight position): a
//! note starts when the grooved position reaches it, so grid-aligned notes
//! start at their shifted times and off-grid notes slide proportionally inside
//! their step, while note lengths stay measured in straight time.

mod follower;
mod template;

pub use follower::{GrooveFollower, GrooveHold, before_first_step};
pub use template::{GrooveTemplate, fill_groove_inputs};

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{MonoSignal, param_errors::ModuleParamErrors};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = validate_groove_params -> ModuleParamErrors)]
struct GrooveParams {
    /// groove template: timing offsets, velocities and grid
    template: GrooveTemplate,
    /// straight playhead to groove (defaults to the global clock, ignoring the global groove)
    #[default_connection(module = RootClock, port = "playhead", channels = [0, 1])]
    #[signal(range = (0.0, 1.0))]
    #[deserr(default)]
    playhead: Option<MonoSignal>,
}

fn validate_groove_params(
    params: GrooveParams,
    _location: deserr::ValuePointerRef,
) -> Result<GrooveParams, ModuleParamErrors> {
    if let Err(msg) = params.template.validate() {
        let mut err = ModuleParamErrors::default();
        err.add("template".to_string(), msg);
        return Err(err);
    }
    Ok(params)
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GrooveOutputs {
    #[output(
        "offset",
        "grooved minus straight playhead position, in cycles",
        default
    )]
    offset: f32,
    #[output("velocity", "velocity of the current groove step (0-5V)", range = (0.0, 5.0))]
    velocity: f32,
    #[output("trigger", "5V trigger at the start of each grooved step", range = (0.0, 5.0))]
    trigger: f32,
}

/// Groove a playhead with a timing and velocity template.
///
/// The template moves each grid step by its timing offset. A `$cycle` given
/// this module's offset starts grid-aligned notes at the shifted times, slides
/// notes between grid steps proportionally, and keeps every note's length.
/// Every `$cycle` follows the global groove (see `$setGroove`) by default;
/// pass this module's offset and velocity to give one its own groove.
///
/// Timing offsets are in grid steps: with `grid: 16`, an offset of `0.2` delays
/// that sixteenth by a fifth of a sixteenth. The template repeats every
/// `timing.length` steps.
///
/// ```js
/// const g = $groove({ timing: [0, 0.3], velocity: [1, 0.6] })
/// const seq = $cycle($p('c4*8'), { groove: g, velocity: g.velocity })
/// $sine(seq).amp($adsr(seq.gate)).amp(seq.velocity).out()
/// ```
#[module(name = "$groove", args(template))]
pub struct Groove {
    outputs: GrooveOutputs,
    params: GrooveParams,
    state: GrooveFollower,
}

impl Groove {
    fn update(&mut self, sample_rate: f32) {
        let straight = self
            .params
            .playhead
            .as_ref()
            .map_or(0.0, MonoSignal::get_value_f64);
        let frame = self
            .state
            .advance(&self.params.template, straight, sample_rate);
        self.outputs.offset = frame.offset;
        self.outputs.velocity = frame.velocity;
        self.outputs.trigger = frame.trigger;
    }
}

message_handlers!(impl Groove {});
