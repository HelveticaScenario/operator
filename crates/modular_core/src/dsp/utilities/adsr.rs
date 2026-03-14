use crate::dsp::utils::SchmittTrigger;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct AdsrParams {
    /// gate input — rising edge starts the envelope, falling edge triggers release
    #[signal(type = gate, range = (0.0, 5.0))]
    gate: PolySignal,
    /// attack time in seconds
    #[signal(default = 0.01, range = (0.0, 10.0))]
    attack: Option<PolySignal>,
    /// decay time in seconds
    #[signal(default = 0.1, range = (0.0, 10.0))]
    decay: Option<PolySignal>,
    /// sustain level in volts (0-5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    sustain: Option<PolySignal>,
    /// release time in seconds
    #[signal(default = 0.1, range = (0.0, 10.0))]
    release: Option<PolySignal>,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum EnvelopeStage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Per-channel envelope state
#[derive(Clone, Copy)]
struct ChannelState {
    stage: EnvelopeStage,
    current_level: f32,
    gate_schmitt: SchmittTrigger,
    attack: f32,
    decay: f32,
    release: f32,
    sustain: f32,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            stage: EnvelopeStage::Idle,
            current_level: 0.0,
            gate_schmitt: SchmittTrigger::default(),
            attack: 0.01,
            decay: 0.1,
            release: 0.1,
            sustain: 3.5,
        }
    }
}

/// State for the Adsr module.
#[derive(Default)]
struct AdsrState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// An Attack-Decay-Sustain-Release envelope generator.
///
/// Generates a control voltage envelope driven by a **gate** input.
/// When the gate goes high (>1V) the envelope enters the attack phase;
/// when the gate goes low it enters release.
///
/// - **attack** / **decay** / **release** — time in seconds
/// - **sustain** — level in volts (0–5V)
///
/// Output range is **0–5V**.
///
/// ## Example
///
/// ```js
/// const env = $adsr($pPulse($clock[0]), { attack: 0.01, decay: 0.2, sustain: 3, release: 0.5 })
/// $sine('c4').amplitude(env).out()
/// ```
#[module(name = "$adsr", args(gate))]
pub struct Adsr {
    outputs: AdsrOutputs,
    state: AdsrState,
    params: AdsrParams,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct AdsrOutputs {
    #[output("output", "envelope output", default, range = (0.0, 5.0))]
    sample: PolyOutput,
}

impl Adsr {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            // Smooth parameter targets to avoid clicks when values change (times in seconds)
            state.attack = self.params.attack.value_or(ch, 0.01).max(0.001);
            state.decay = self.params.decay.value_or(ch, 0.1).max(0.001);
            state.release = self.params.release.value_or(ch, 0.1).max(0.001);
            state.sustain = self.params.sustain.value_or(ch, 5.).max(0.0);

            let attack = state.attack;
            let decay = state.decay;
            let release_var = state.release;

            let gate_val = self.params.gate.get_value(ch);
            let (gate_on, edge) = state.gate_schmitt.process_with_edge(gate_val);

            if edge.is_rising() {
                state.stage = EnvelopeStage::Attack;
            } else if edge.is_falling() {
                state.stage = EnvelopeStage::Release;
            }

            let sustain_level = (state.sustain / 5.0).clamp(0.0, 1.0);

            match state.stage {
                EnvelopeStage::Idle => {
                    state.current_level = 0.0;
                }
                EnvelopeStage::Attack => {
                    if attack < 0.0001 {
                        state.current_level = 1.0;
                        state.stage = EnvelopeStage::Decay;
                    } else {
                        let step = 1.0 / (attack * sample_rate);
                        state.current_level += step;
                        if state.current_level >= 1.0 {
                            state.current_level = 1.0;
                            state.stage = EnvelopeStage::Decay;
                        }
                    }
                }
                EnvelopeStage::Decay => {
                    if decay <= 0.0001 || state.current_level <= sustain_level {
                        state.current_level = sustain_level;
                        state.stage = EnvelopeStage::Sustain;
                    } else {
                        let step = (1.0 - sustain_level) / (decay * sample_rate);
                        state.current_level = (state.current_level - step).max(sustain_level);
                        if state.current_level <= sustain_level {
                            state.current_level = sustain_level;
                            state.stage = EnvelopeStage::Sustain;
                        }
                    }
                }
                EnvelopeStage::Sustain => {
                    state.current_level = sustain_level;
                    if !gate_on {
                        state.stage = EnvelopeStage::Release;
                    }
                }
                EnvelopeStage::Release => {
                    if release_var <= 0.0001 {
                        state.current_level = 0.0;
                        state.stage = EnvelopeStage::Idle;
                    } else {
                        let step = state.current_level / (release_var * sample_rate);
                        state.current_level = (state.current_level - step).max(0.0);
                        if state.current_level <= 0.00001 {
                            state.current_level = 0.0;
                            state.stage = if gate_on {
                                EnvelopeStage::Attack
                            } else {
                                EnvelopeStage::Idle
                            };
                        }
                    }
                }
            }

            self.outputs.sample.set(ch, state.current_level * 5.0);
        }
    }
}

message_handlers!(impl Adsr {});
