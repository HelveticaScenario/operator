use deserr::Deserr;
use mi_plaits_dsp::utils::filter::{FilterMode, FrequencyApproximation, Svf};
use schemars::JsonSchema;

use crate::{
    dsp::utils::SchmittTrigger,
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
};

/// Plaits' coefficients are tuned for its 48 kHz engine; frequencies below
/// are normalized to this rate and rescaled to the engine rate.
const PLAITS_SAMPLE_RATE: f32 = 48000.0;

/// Plaits' vactrol rises with a coefficient of 0.6 per 12-sample block at
/// 48 kHz, i.e. a continuous rate of `-ln(0.4) / (12 / 48000)` per second.
const VACTROL_RISE_RATE: f32 = 3665.16;

/// A pinged vactrol ramps up over half a period of the oscillator note in
/// Plaits; with no pitch here, the ramp uses Plaits' rate at C4.
const PING_ATTACK_HZ: f32 = 2.0 * 261.63;

const SVF_Q: f32 = 0.4;

/// Highest filter cutoff as a fraction of the engine rate; keeps the
/// Plaits frequency curve below Nyquist at low engine rates.
const MAX_NORMALIZED_CUTOFF: f32 = 0.45;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct LpgParams {
    /// signal input
    input: PolySignal,
    /// trigger input (rising edge pings the vactrol fully open)
    #[signal(type = trig, range = (0.0, 5.0))]
    #[deserr(default)]
    trigger: Option<PolySignal>,
    /// level CV (0-5V) - the floor pings rise from and decay back to; 5V fully open
    #[signal(range = (0.0, 5.0))]
    #[deserr(default)]
    level: Option<PolySignal>,
    /// filter response (0-5) - low = mellow lowpass gate, high = bright, VCA-like
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    color: Option<PolySignal>,
    /// vactrol decay (0-5) - low = short pluck, high = long ring
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LpgOutputs {
    #[output("output", "gated signal", default)]
    sample: PolyOutput,
}

#[derive(Clone, Default)]
struct LpgChannel {
    vactrol: f32,
    ramp_up: bool,
    trigger: SchmittTrigger,
    color: Clickless,
    filter: Svf,
}

/// Sample-rate-derived coefficients, computed once in `init`.
#[derive(Default)]
struct LpgState {
    inv_sample_rate: f32,
    rise: f32,
    ping_attack: f32,
}

/// Vactrol low pass gate modeled on the one built into Mutable Instruments Plaits.
///
/// The gate is a filter and VCA driven together by a simulated vactrol: as it
/// opens, the signal gets both louder and brighter, and it closes with the
/// slow, non-linear tail that gives plucked and struck sounds their bounce.
///
/// - **trigger** — each rising edge pings the vactrol fully open, then it
///   decays back to **level**.
/// - **level** — the floor the vactrol rests at (0–5 V, 5 V fully open). With
///   no trigger it works as a CV-controlled gate; at half open, a ping swings
///   the gate from half open to fully open and back.
/// - **color** — low values close the filter as the gate closes (mellow,
///   woody); high values let highs through, approaching a plain VCA.
/// - **decay** — length of the vactrol's release.
///
/// With neither **trigger** nor **level** patched the gate is bypassed and
/// the input passes through unchanged.
///
/// ```js
/// // plucked saw pinged on every beat
/// $lpg($saw('c3'), $clock.beatTrigger, { color: 1.5, decay: 2 }).out()
/// ```
#[module(name = "$lpg", args(input, trigger), has_init)]
pub struct Lpg {
    outputs: LpgOutputs,
    state: LpgState,
    channel_state: Box<[LpgChannel]>,
    params: LpgParams,
}

impl Lpg {
    fn init(&mut self, sample_rate: f32) {
        self.state.inv_sample_rate = 1.0 / sample_rate;
        self.state.rise = 1.0 - (-VACTROL_RISE_RATE / sample_rate).exp();
        self.state.ping_attack = PING_ATTACK_HZ / sample_rate;
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let level_patched = self.params.level.is_some();
        let trigger_patched = self.params.trigger.is_some();

        if !level_patched && !trigger_patched {
            for ch in 0..channels {
                self.outputs.sample.set(ch, self.params.input.get_value(ch));
            }
            return;
        }

        let inv_sr = self.state.inv_sample_rate;
        for ch in 0..channels {
            let cs = &mut self.channel_state[ch];

            cs.color
                .update(self.params.color.value_or(ch, 2.5).clamp(0.0, 5.0) / 5.0);
            let hf = *cs.color;

            let decay = self.params.decay.value_or(ch, 2.5).clamp(0.0, 5.0) / 5.0;

            let short_decay = 200.0 * inv_sr * (-8.0 * decay.max(0.1)).exp2();
            let decay_tail = 20.0 * inv_sr * (hf - 6.0 * decay).exp2() - short_decay;

            let l = self.params.level.value_or(ch, 0.0) / 5.0;
            let floor = (1.3 * l / (0.3 + l.abs())).clamp(0.0, 1.0);

            if cs.trigger.process(self.params.trigger.value_or(ch, 0.0)) {
                cs.ramp_up = true;
            }
            if cs.ramp_up {
                cs.vactrol += self.state.ping_attack;
                if cs.vactrol >= 1.0 {
                    cs.vactrol = 1.0;
                    cs.ramp_up = false;
                }
            }
            let target = if cs.ramp_up {
                cs.vactrol.max(floor)
            } else {
                floor
            };

            let error = target - cs.vactrol;
            let v2 = cs.vactrol * cs.vactrol;
            let v4 = v2 * v2;
            let tail = 1.0 - cs.vactrol;
            let tail_2 = tail * tail;
            let coefficient = if error > 0.0 {
                self.state.rise
            } else {
                short_decay + (1.0 - v4) * decay_tail
            };
            cs.vactrol += coefficient * error;

            let hf_bleed = (tail_2 + (1.0 - tail_2) * hf) * hf * hf;
            let cutoff_hz = PLAITS_SAMPLE_RATE * (0.003 + 0.3 * v4 + 0.04 * hf);
            let cutoff = (cutoff_hz * inv_sr).min(MAX_NORMALIZED_CUTOFF);
            cs.filter
                .set_f_q(cutoff, SVF_Q, FrequencyApproximation::Dirty);

            let s = self.params.input.get_value(ch) * cs.vactrol;
            let lp = cs.filter.process(s, FilterMode::LowPass);
            self.outputs.sample.set(ch, lp + (s - lp) * hf_bleed);
        }
    }
}

message_handlers!(impl Lpg {});
