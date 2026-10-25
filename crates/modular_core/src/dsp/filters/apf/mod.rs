mod channel;
mod feedback;
mod stage;

use std::ops::Range;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::{C4_HZ_F32, rng::seed_base},
    param_errors::ModuleParamErrors,
    poly::{PolyOutput, PolySignal, PolySignalExt},
};

use channel::{AllpassChannel, Controls};

const MAX_STAGES: usize = 50;

/// Largest `fadeStep`: one crossfade spanning the 1- and 50-stage chains.
const MAX_FADE_STEP: u32 = MAX_STAGES as u32 - 1;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = validate_allpass_params -> ModuleParamErrors)]
struct AllpassParams {
    /// signal input
    input: PolySignal,
    /// frequency in V/Oct (0V = C4) the stages delay most
    #[signal(type = frequency, default = 0.0, range = (-5.0, 5.0))]
    cutoff: PolySignal,
    /// number of two-pole stages (1-50): how much smear
    #[signal(default = 1.0, range = (1.0, 50.0))]
    #[deserr(default)]
    stages: Option<PolySignal>,
    /// how sharply the stages ring at the cutoff (0-5)
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    resonance: Option<PolySignal>,
    /// output fed back into the input (-5 to 5, up to ±6 to self-oscillate)
    #[signal(type = control, default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    feedback: Option<PolySignal>,
    /// stages between crossfaded chains (0-49); 0 glides instead
    #[serde(default)]
    #[deserr(default)]
    fade_step: u32,
}

fn validate_allpass_params(
    params: AllpassParams,
    _location: deserr::ValuePointerRef,
) -> Result<AllpassParams, ModuleParamErrors> {
    if params.fade_step > MAX_FADE_STEP {
        let mut err = ModuleParamErrors::default();
        err.add(
            "fadeStep".to_string(),
            format!("must be from 0 to {MAX_FADE_STEP}"),
        );
        return Err(err);
    }
    Ok(params)
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct AllpassOutputs {
    #[output("output", "phase-shifted signal", default)]
    sample: PolyOutput,
}

/// Stacked allpass filter: 1 to 50 two-pole stages, with resonance and
/// feedback. Smears transients into chirps around the cutoff; mixed with the
/// dry signal, it notches like a phaser.
///
/// - **cutoff** — V/Oct, the frequency delayed most.
/// - **stages** — 1–50. More stages, longer smear. A fractional count fades
///   the next stage in smoothly.
/// - **resonance** — 0–5. Makes the smear ring at the cutoff's pitch.
/// - **feedback** — −5 to 5. Repeats the smear; rings get longer toward ±5,
///   where they never fade. Up to ±6 the filter self-oscillates. Negative
///   values flip the sign of every other repeat.
/// - **fadeStep** — 0–49. Above 0, `stages` crossfades between chains this
///   many stages apart instead, giving notches.
///
/// ```js
/// // smear a click on every beat into a chirp around C3
/// $pPulse($clock[0]).$.apf('c3', { stages: 50 }).out()
/// // ringing, pitched smear with repeats
/// $pPulse($clock[0]).$.apf('g3', { stages: 32, resonance: 3, feedback: 3 }).out()
/// // self-oscillating: no input, just the loop
/// $apf(0, 'c3', { stages: 20, resonance: 2, feedback: 5.5 }).out()
/// // notches: crossfading between chains 8 stages apart
/// $noise('white').$.apf('c4', { stages: $sine('0.05hz').range(9, 25), resonance: 2, fadeStep: 8 }).out()
/// // four-stage phaser: swept allpass mixed equally with the dry saw
/// $saw('c2').$m.apf(2.5, $sine('0.2hz').range(-1, 3), { stages: 4, resonance: 2, feedback: 3 }).out()
/// ```
#[module(name = "$apf", args(input, cutoff), has_init, block)]
pub struct Allpass {
    outputs: AllpassOutputs,
    state: AllpassState,
    channel_state: Box<[AllpassChannel]>,
    params: AllpassParams,
}

#[derive(Default)]
struct AllpassState {
    /// Converts a V/Oct cutoff to octaves relative to the sample rate.
    log2_c4_over_fs: f32,
}

impl Allpass {
    fn init(&mut self, sample_rate: f32) {
        self.state.log2_c4_over_fs = (C4_HZ_F32 / sample_rate).log2();
        let base = seed_base(self);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            state.seed_noise(base, ch);
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let fade_step = self.params.fade_step as usize;
        let p = &self.params;
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            let controls = Controls {
                cutoff: p.cutoff.get_value(ch),
                stages: p.stages.value_or(ch, AllpassParams::STAGES_DEFAULT),
                resonance: p.resonance.value_or(ch, AllpassParams::RESONANCE_DEFAULT),
                feedback: p.feedback.value_or(ch, AllpassParams::FEEDBACK_DEFAULT),
            };
            let y = state.step(
                p.input.get_value(ch),
                controls,
                fade_step,
                self.state.log2_c4_over_fs,
            );
            self.outputs.sample.set(ch, y);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut AllpassBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let fade_step = self.params.fade_step as usize;
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), self.channel_count(), &mut scratch)?;
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            for i in range.clone() {
                let controls = Controls {
                    cutoff: inputs.cutoff.row(i)[ch],
                    stages: inputs.stages.row(i)[ch],
                    resonance: inputs.resonance.row(i)[ch],
                    feedback: inputs.feedback.row(i)[ch],
                };
                let y = state.step(
                    inputs.input.row(i)[ch],
                    controls,
                    fade_step,
                    self.state.log2_c4_over_fs,
                );
                outputs.sample.slot_mut(i)[ch] = y;
            }
        }
        Some(())
    }
}

message_handlers!(impl Allpass {});

#[cfg(test)]
mod tests;
