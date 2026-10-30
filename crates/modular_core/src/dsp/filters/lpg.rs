use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::{
        mi::core::filter::{Svf, tan_dirty},
        utils::{SchmittTrigger, changed},
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
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

/// Time constant of the color smoother.
const COLOR_SMOOTHING_SECONDS: f32 = 0.002;

/// Length of the crossfade between bypass and the gated signal.
const BYPASS_FADE_SECONDS: f32 = 0.005;

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
    /// level CV (0-5V) - the floor pings rise from and decay back to, on Plaits' compressing curve; 5V fully open
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

#[derive(Clone)]
struct LpgChannel {
    /// False until the channel has processed a sample.
    running: bool,
    vactrol: f32,
    ramp_up: bool,
    trigger: SchmittTrigger,
    color: f32,
    /// 1 = input passes through, 0 = gated signal.
    bypass_mix: f32,
    filter: Svf,
    /// Decay coefficients for the cached `decay`/`color` pair; `NaN` = never computed.
    last_decay: f32,
    last_color: f32,
    short_decay: f32,
    decay_tail: f32,
}

impl Default for LpgChannel {
    fn default() -> Self {
        Self {
            running: false,
            vactrol: 0.0,
            ramp_up: false,
            trigger: SchmittTrigger::default(),
            color: 0.0,
            bypass_mix: 0.0,
            filter: Svf::default(),
            last_decay: f32::NAN,
            last_color: f32::NAN,
            short_decay: 0.0,
            decay_tail: 0.0,
        }
    }
}

#[derive(Default)]
struct LpgState {
    /// False until the module has processed a sample. Channels added to a
    /// running module start from the current trigger level instead of
    /// treating a held-high trigger as a rising edge.
    running: bool,
    rise: f32,
    ping_attack: f32,
    color_smoothing: f32,
    bypass_fade_step: f32,
}

/// Vactrol low pass gate modeled on the one built into Mutable Instruments Plaits.
///
/// The gate is a filter and VCA driven together by a simulated vactrol: as it
/// opens, the signal gets both louder and brighter, and it closes with the
/// slow, non-linear tail that gives plucked and struck sounds their bounce.
///
/// - **trigger** — each rising edge pings the vactrol fully open, then it
///   decays back to **level**.
/// - **level** — the floor the vactrol rests at, 0–5 V on Plaits' compressing
///   curve: about 0.9 V opens it halfway, 2.5 V about 80%, 5 V fully. With no
///   trigger it works as a CV-controlled gate; with a trigger, each ping
///   swings the gate from the floor to fully open and back.
/// - **color** — low values close the filter as the gate closes (mellow,
///   woody); high values let highs through, approaching a plain VCA.
/// - **decay** — length of the vactrol's release.
///
/// With neither **trigger** nor **level** patched the gate is bypassed and
/// the input passes through unchanged; patching or unpatching them
/// crossfades over 5 ms.
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
        self.state.rise = 1.0 - (-VACTROL_RISE_RATE / sample_rate).exp();
        self.state.ping_attack = PING_ATTACK_HZ / sample_rate;
        self.state.color_smoothing = 1.0 - (-1.0 / (COLOR_SMOOTHING_SECONDS * sample_rate)).exp();
        self.state.bypass_fade_step = 1.0 / (BYPASS_FADE_SECONDS * sample_rate);
    }

    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let bypassed = self.params.level.is_none() && self.params.trigger.is_none();
        let bypass_target = if bypassed { 1.0 } else { 0.0 };
        let inv_sr = 1.0 / sample_rate;
        let module_running = self.state.running;

        for ch in 0..channels {
            let cs = &mut self.channel_state[ch];

            let color = self.params.color.value_or(ch, 2.5).clamp(0.0, 5.0) / 5.0;
            let trigger = self.params.trigger.value_or(ch, 0.0);
            let rising = cs.trigger.process(trigger);
            if cs.running {
                cs.color += (color - cs.color) * self.state.color_smoothing;
                cs.bypass_mix += (bypass_target - cs.bypass_mix)
                    .clamp(-self.state.bypass_fade_step, self.state.bypass_fade_step);
            } else {
                cs.color = color;
                cs.bypass_mix = if module_running { 0.0 } else { bypass_target };
            }
            if rising && (cs.running || !module_running) {
                cs.ramp_up = true;
            }
            cs.running = true;

            let hf = cs.color;
            let decay = self.params.decay.value_or(ch, 2.5).clamp(0.0, 5.0) / 5.0;
            if changed(decay, cs.last_decay) || changed(hf, cs.last_color) {
                cs.last_decay = decay;
                cs.last_color = hf;
                cs.short_decay = 200.0 * inv_sr * (-8.0 * decay).exp2();
                cs.decay_tail = 20.0 * inv_sr * (hf - 6.0 * decay).exp2() - cs.short_decay;
            }

            let l = self.params.level.value_or(ch, 0.0) / 5.0;
            let floor = (1.3 * l / (0.3 + l.abs())).clamp(0.0, 1.0);

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
                cs.short_decay + (1.0 - v4) * cs.decay_tail
            };
            cs.vactrol += coefficient * error;

            let hf_bleed = (tail_2 + (1.0 - tail_2) * hf) * hf * hf;
            let cutoff_hz = PLAITS_SAMPLE_RATE * (0.003 + 0.3 * v4 + 0.04 * hf);
            let cutoff = (cutoff_hz * inv_sr).min(MAX_NORMALIZED_CUTOFF);
            cs.filter.set_g_q(tan_dirty(cutoff), SVF_Q);

            let input = self.params.input.get_value(ch);
            let s = input * cs.vactrol;
            let lp = cs.filter.low_pass(s);
            let gated = lp + (s - lp) * hf_bleed;
            self.outputs
                .sample
                .set(ch, gated + (input - gated) * cs.bypass_mix);
        }
        self.state.running = true;
    }
}

message_handlers!(impl Lpg {});
