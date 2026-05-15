use crate::dsp::utils::SchmittTrigger;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
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
}

/// State for the PercussionEnvelope module.
#[derive(Default)]
struct PercussionEnvelopeState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
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
/// $noise("white").mul($perc($clock.gate, { decay: 0.1 }))
/// ```
#[module(name = "$perc", args(trigger))]
pub struct PercussionEnvelope {
    outputs: PercussionEnvelopeOutputs,
    params: PercussionEnvelopeParams,
    state: PercussionEnvelopeState,
}

impl PercussionEnvelope {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let decay_time = self.params.decay.value_or(ch, 0.1).max(0.001);

            // Detect rising edge of trigger using Schmitt trigger for noise immunity
            let trigger = self.params.trigger.get_value(ch);
            if state.trigger_schmitt.process(trigger) {
                // Trigger detected - start attack phase (continue from current level for smooth re-trigger)
                state.in_attack = true;
            }

            // Attack phase: 1ms linear ramp to peak
            if state.in_attack {
                const ATTACK_TIME: f32 = 0.001; // 1ms
                let step = 1.0 / (ATTACK_TIME * sample_rate);
                state.current_level += step;
                if state.current_level >= 1.0 {
                    state.current_level = 1.0;
                    state.in_attack = false;
                }
            } else if state.current_level > 0.00001 {
                // Exponential decay
                // Calculate decay coefficient for exponential decay
                // Time constant tau = decay_time, we want level to reach ~0.001 after decay_time
                // Using e^(-t/tau) where tau = decay_time / 6.9 (ln(1000) ≈ 6.9)
                let tau = decay_time / 6.9;
                let decay_coeff = (-1.0 / (tau * sample_rate)).exp();
                state.current_level *= decay_coeff;
            } else {
                state.current_level = 0.0;
            }

            self.outputs.sample.set(ch, state.current_level * 5.0);
        }
    }
}

message_handlers!(impl PercussionEnvelope {});
