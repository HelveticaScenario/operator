use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{Decay, Strike, Svf, sine};
use crate::dsp::utils::{voct_to_hz, wrap_phase};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// D5 (≈587 Hz) in V/Oct.
const DEFAULT_FREQ: f32 = 14.0 / 12.0;
const DEFAULT_DECAY: f32 = 1.33;
const DEFAULT_FEEDBACK: f32 = 2.0;
const DEFAULT_MODULATION: f32 = 4.6;
const DEFAULT_TONE: f32 = 1.4;

/// Carrier self-modulation at full `feedback`, in cycles of phase.
const FEEDBACK_MAX: f32 = 1.0;
/// The modulator swings the carrier's frequency by `DEVIATION_MAX_HZ · a³`
/// for `amount` a in 0–1; deep settings drive it through zero.
const DEVIATION_MAX_HZ: f32 = 74_000.0;
/// The modulator's rate in Hz at full `modulation`.
const MOD_RATE_MAX_HZ: f32 = 100.0;
/// The hit fades in over about this many seconds.
const ATTACK_SECS: f32 = 0.0002;
/// A low-pass closes over the hit from `CLOSE_HZ`, falling an octave every
/// `CLOSE_OCTAVE` of `decay`, so the carrier dulls and then fades while the
/// low rumble of heavy feedback lingers.
const CLOSE_HZ: f32 = 20_000.0;
const CLOSE_OCTAVE: f32 = 0.09;
/// `tone` t (0–1) sets a low-pass at `TONE_BASE_HZ + TONE_SPAN_HZ · t` and
/// mixes back t² of the unfiltered carrier.
const TONE_BASE_HZ: f32 = 250.0;
const TONE_SPAN_HZ: f32 = 10_000.0;
/// Level of the softest accent.
const ACCENT_FLOOR: f32 = 0.087;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct FmDrumParams {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// carrier pitch in V/Oct (default D5, ≈587 Hz)
    #[signal(type = pitch, default = 1.167, range = (-3.0, 5.0))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the hit to fall to −60 dB; a closing low-pass ends it
    /// sooner (default 1.33)
    #[signal(default = 1.33, range = (0.0, 5.5))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// carrier self-modulation, 0–5 V: adds harmonics, then turns to noise
    /// above about 2 V (default 2)
    #[signal(default = 2.0, range = (0.0, 5.0))]
    #[deserr(default)]
    feedback: Option<PolySignal>,
    /// depth of frequency modulation, 0–5 V, rising steeply from a gentle
    /// wobble to swings of tens of kHz (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    amount: Option<PolySignal>,
    /// rate of the frequency modulation, 0–5 V for 0–100 Hz (default 4.6)
    #[signal(default = 4.6, range = (0.0, 5.0))]
    #[deserr(default)]
    modulation: Option<PolySignal>,
    /// brightness, 0–5 V: a low-pass that opens and lets the raw carrier
    /// through (default 1.4)
    #[signal(default = 1.4, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// hit strength, 0–5 V, sampled on each trigger and held for the hit; it
    /// scales the output level only (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct FmDrumOutputs {
    #[output("output", "drum output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    strike: Strike,
    env: Decay,
    attack: f32,
    /// Seconds since the strike.
    elapsed: f32,
    carrier_phase: f32,
    /// The modulator runs freely, so each hit catches it at a new point.
    modulator_phase: f32,
    last: f32,
    tone: Svf,
    close: Svf,
}

#[derive(Default)]
struct FmDrumState {
    attack_coeff: f32,
    sample_period: f32,
}

/// FM drum: a two-operator FM voice with carrier feedback, for metallic toms,
/// zaps and bells.
///
/// Each rising edge on **trigger** strikes a sine carrier at **freq** whose
/// frequency is swung by a sine of up to 100 Hz (set by **modulation**) with
/// depth **amount**. **feedback** lets the carrier modulate itself, adding
/// harmonics and then noise. **tone** low-passes the result, which falls to
/// −60 dB over **decay** seconds. **accent**, sampled at each strike, sets the
/// hit's level.
///
/// Output range is **±5 V**.
///
/// ```js
/// $fmDrum($clock.beatTrigger, { freq: 'c3', amount: 1.5, decay: 0.4 }).out()
/// ```
#[module(name = "$fmDrum", has_init, args(trigger))]
pub struct FmDrum {
    outputs: FmDrumOutputs,
    params: FmDrumParams,
    state: FmDrumState,
    channel_state: Box<[ChannelState]>,
}

impl FmDrum {
    fn init(&mut self, sample_rate: f32) {
        self.state.attack_coeff = 1.0 - (-1.0 / (ATTACK_SECS * sample_rate)).exp();
        self.state.sample_period = 1.0 / sample_rate;
    }

    fn update(&mut self, sample_rate: f32) {
        let state = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];

            let modulation = p
                .modulation
                .value_or(ch, DEFAULT_MODULATION)
                .clamp(0.0, 5.0)
                / 5.0;
            cs.modulator_phase =
                wrap_phase(cs.modulator_phase + MOD_RATE_MAX_HZ * modulation / sample_rate);

            if cs
                .strike
                .process(p.trigger.get_value(ch), p.accent.value_or(ch, 5.0))
            {
                cs.env.strike();
                cs.attack = 0.0;
                cs.elapsed = 0.0;
                cs.carrier_phase = 0.0;
                cs.last = 0.0;
            }
            if !cs.env.active() {
                self.outputs.sample.set(ch, 0.0);
                continue;
            }

            let decay = p.decay.value_or(ch, DEFAULT_DECAY).max(1.0e-3);
            let level = cs.env.next(decay, sample_rate);
            let close = CLOSE_HZ * (-cs.elapsed / (CLOSE_OCTAVE * decay)).exp2();
            cs.attack += (1.0 - cs.attack) * state.attack_coeff;
            cs.elapsed += state.sample_period;

            let hz = voct_to_hz(p.freq.value_or(ch, DEFAULT_FREQ));
            let amount = p.amount.value_or(ch, 0.0).clamp(0.0, 5.0) / 5.0;
            let deviation = DEVIATION_MAX_HZ * amount * amount * amount;
            let swung = hz + deviation * sine(cs.modulator_phase);
            cs.carrier_phase = wrap_phase(cs.carrier_phase + swung / sample_rate);

            let feedback = p.feedback.value_or(ch, DEFAULT_FEEDBACK).clamp(0.0, 5.0) / 5.0;
            let carrier = sine(wrap_phase(
                cs.carrier_phase + FEEDBACK_MAX * feedback * cs.last,
            ));
            cs.last = carrier;

            let tone = p.tone.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let cutoff = TONE_BASE_HZ + TONE_SPAN_HZ * tone;
            let low = cs.tone.process(carrier, cutoff, 0.707, sample_rate).lp;
            let dry = tone * tone;
            let toned = (1.0 - dry) * low + dry * carrier;
            let x = cs.close.process(toned, close, 0.707, sample_rate).lp;

            self.outputs.sample.set(
                ch,
                x * level * cs.attack * cs.strike.gain(ACCENT_FLOOR) * 5.0,
            );
        }
    }
}

message_handlers!(impl FmDrum {});
