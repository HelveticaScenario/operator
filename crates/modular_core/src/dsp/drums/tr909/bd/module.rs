use super::*;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Bd909Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the accent voltage, for a hit up to twice as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tune control, 0–5 V: how long the pitch takes to fall to rest
    /// (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tune: Option<PolySignal>,
    /// attack control, 0–5 V: the level of the click (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    attack: Option<PolySignal>,
    /// decay control, 0–5 V: how long the drum rings (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the envelopes, as
    /// on the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Bd909Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-909 bass drum.
///
/// Each rising edge on **trigger** fires the 909's trigger into a circuit
/// model of its bass drum: a triangle VCO rounded toward a sine, its pitch
/// swept down from about 300 Hz to rest near 56 Hz, with a click and a
/// burst of noise at the attack. **tune** sets how long the sweep takes,
/// **attack** the click's level, **decay** how long the drum rings, and
/// **accent**, sampled at each strike, drives it harder.
///
/// An unaccented hit peaks near **±5 V**.
///
/// ```js
/// $tr909.bd($clock.beatTrigger).out()
/// ```
#[module(name = "$tr909.bd", has_init, patch_update, args(trigger))]
pub struct Bd909 {
    outputs: Bd909Outputs,
    params: Bd909Params,
    state: BdState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct BdState {
    shared: DrumShared,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Drum>,
    noise: Noise,
    tune: TaperedPot,
    decay: TaperedPot,
}

impl Bd909 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.noise_rate = Noise::rate(sample_rate);
        sh.envelope = envelope_network(dt);
        // The attack network at DC: discretised at a step long enough that
        // its capacitors carry no current.
        let dc = attack_network(DC_STEP);
        let mut injected = [0.0f32; ATTACK_UNKNOWNS];
        injected[NOISE_IN] = NOISE_MEAN * STIFF;
        injected[PE] = -RAIL / 10.0e3;
        let mut rest = MnaState::default();
        for _ in 0..DC_ITERATIONS {
            dc.step(&mut rest, 1 << Q8_ON, &injected);
        }
        sh.attack_rest = rest;
        sh.attack = attack_network(dt);
        sh.sync_coeff = (-dt / (R51 * C14)).exp();
        sh.c7_charge = (-dt / (R20 * C7)).exp();
        sh.c7_bleed = (-dt / (R18 * C7)).exp();
        sh.c12_charge = (-(1.0 / R41 + 1.0 / R493) * dt / C12).exp();
        sh.c12_bleed = (-dt / (R493 * C12)).exp();
    }

    fn update(&mut self, _sample_rate: f32) {
        let sh = &self.state.shared;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let knob = |s: &Option<PolySignal>, default: f32| {
                s.value_or(ch, default).clamp(0.0, 5.0) / 5.0
            };
            let controls = DrumControls {
                tune: cs.tune.conductance(knob(&p.tune, DEFAULT_TUNE), R57, VR2),
                decay: cs
                    .decay
                    .conductance(knob(&p.decay, DEFAULT_DECAY), R58, VR5),
                attack: VR3 * (1.0 - knob(&p.attack, DEFAULT_ATTACK)),
                noise: cs.noise.next(sh.noise_rate),
            };
            let out = cs.voices.process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                sh.timing.choke_samples,
                sh,
                &controls,
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

impl crate::types::PatchUpdateHandler for Bd909 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.noise.seed(base, ch);
        }
    }
}

message_handlers!(impl Bd909 {});
