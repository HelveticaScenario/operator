use std::ops::Range;

use crate::dsp::utils::{SchmittTrigger, sanitize};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct PercussionEnvelopeParams {
    /// trigger input (rising edge triggers envelope)
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// decay time in seconds
    #[signal(default = 0.1, range = (0.0, 10.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PercussionEnvelopeOutputs {
    #[output("output", "envelope output", default, range = (0.0, 5.0))]
    sample: PolyOutput,
}

/// Per-channel envelope state
#[derive(Default, Clone, Copy)]
struct ChannelState {
    current_level: f32,
    trigger_schmitt: SchmittTrigger,
    in_attack: bool,
    /// The decay time the cached coefficient was computed for, and the
    /// coefficient.
    decay_cache: Option<(f32, f32)>,
}

/// Simple envelope for percussive sounds.
///
/// A rising edge on **trigger** starts the envelope at 5 V, which then
/// decays exponentially to 0 V over the **decay** time. Perfect for
/// hi-hats, kicks, and other transient sounds.
///
/// Output range is **0–5 V**.
///
/// ```js
/// // short percussive hit
/// $noise("white").amplitude($perc($clock.beatTrigger, { decay: 0.1 })).out()
/// ```
#[module(name = "$perc", args(trigger), block)]
pub struct PercussionEnvelope {
    outputs: PercussionEnvelopeOutputs,
    params: PercussionEnvelopeParams,
    channel_state: Box<[ChannelState]>,
}

impl ChannelState {
    /// Advance one sample and return the envelope output. `decay` is the decay
    /// time in seconds and `attack_step` the per-sample attack rise.
    #[inline(always)]
    fn step(&mut self, trigger: f32, decay: f32, attack_step: f32, sample_rate: f32) -> f32 {
        // Detect rising edge of trigger using Schmitt trigger for noise immunity
        if self.trigger_schmitt.process(trigger) {
            // Trigger detected - start attack phase (continue from current level for smooth re-trigger)
            self.in_attack = true;
        }

        if self.in_attack {
            self.current_level += attack_step;
            if self.current_level >= 1.0 {
                self.current_level = 1.0;
                self.in_attack = false;
            }
        } else if self.current_level > 0.00001 {
            self.current_level *= self.decay_coeff(decay, sample_rate);
        } else {
            self.current_level = 0.0;
        }
        self.current_level * 5.0
    }

    /// Per-sample multiplier that decays the level to ~0.001 over `decay`
    /// seconds: e^(-t/tau) with tau = decay / 6.9 (ln(1000) ≈ 6.9). Cached
    /// until `decay` changes; the sample rate is fixed for the state's life.
    #[inline(always)]
    fn decay_coeff(&mut self, decay: f32, sample_rate: f32) -> f32 {
        match self.decay_cache {
            Some((cached, coeff)) if cached == decay => coeff,
            _ => {
                let tau = decay.max(0.001) / 6.9;
                let coeff = (-1.0 / (tau * sample_rate)).exp();
                self.decay_cache = Some((decay, coeff));
                coeff
            }
        }
    }
}

/// Per-sample linear rise of the 1 ms attack.
#[inline(always)]
fn attack_step(sample_rate: f32) -> f32 {
    const ATTACK_TIME: f32 = 0.001;
    1.0 / (ATTACK_TIME * sample_rate)
}

impl PercussionEnvelope {
    fn update(&mut self, sample_rate: f32) {
        let p = &self.params;
        let attack_step = attack_step(sample_rate);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            let trigger = p.trigger.get_value(ch);
            let decay = p
                .decay
                .value_or(ch, PercussionEnvelopeParams::DECAY_DEFAULT);
            self.outputs
                .sample
                .set(ch, state.step(trigger, decay, attack_step, sample_rate));
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut PercussionEnvelopeBlockOutputs,
        range: Range<usize>,
        sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let PercussionEnvelopeParamsBlockInputs { trigger, decay } =
            p.block_inputs(range.clone(), channels, &mut scratch)?;
        let attack_step = attack_step(sample_rate);
        for i in range {
            let (trigger, decay) = (trigger.row(i), decay.row(i));
            let slots = outputs
                .sample
                .slot_mut(i)
                .iter_mut()
                .zip(self.channel_state.iter_mut());
            for (ch, (out, state)) in slots.enumerate() {
                *out = sanitize(state.step(trigger[ch], decay[ch], attack_step, sample_rate));
            }
        }
        Some(())
    }
}

message_handlers!(impl PercussionEnvelope {});
