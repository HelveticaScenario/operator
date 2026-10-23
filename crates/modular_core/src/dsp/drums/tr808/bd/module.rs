use super::*;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Bd808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud with
    /// more pitch sigh (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tone control, 0–5 V: opens the output low-pass from ≈300 Hz to ≈7 kHz,
    /// letting through more of the attack click (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// seconds for the ring to fall 60 dB; the stock decay knob spans
    /// ≈0.09–3.8 s, and the circuit can ring no shorter than ≈0.09 s at stock
    /// pitch (default 1)
    #[signal(default = 1.0, range = (0.1, 4.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// resting pitch of the bridged-T in V/Oct, scaling the whole circuit;
    /// stock is ≈49.5 Hz (default -2.4035)
    #[signal(type = pitch, default = -2.4035, range = (-3.5, -1.0))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a still-ringing hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on the
    /// hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Bd808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 bass drum.
///
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its bass drum: a resonant bridged-T filter rings at
/// ≈49.5 Hz with a short frequency jump on the attack and a falling pitch
/// "sigh". **decay** sets how long the ring lasts, **tone** how much of the
/// attack click comes through, and **accent**, sampled at each strike, drives
/// the circuit harder. Retriggering a ringing drum adds to its current state
/// rather than restarting it, as on the hardware, so long decays at fast
/// tempos swing in level from hit to hit; **choke** starts every hit from rest
/// instead.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near
/// **±10 V**; higher **freq** and **tone** run hotter, up to where the
/// circuit's op-amp clips.
///
/// ```js
/// $tr808.bd($clock.beatTrigger, { decay: 2 }).out()
/// ```
#[module(name = "$tr808.bd", has_init, args(trigger))]
pub struct Bd808 {
    outputs: Bd808Outputs,
    params: Bd808Params,
    state: Bd808State,
    channel_state: Box<[ChannelState]>,
}

#[derive(Clone, Copy)]
struct ChannelState {
    voices: Voices<Voice>,
    network: Network,
    /// `tone` the low-pass coefficient is computed for.
    tone: f32,
    tone_coeff: f32,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            voices: Voices::default(),
            network: Network::default(),
            // NaN never compares equal, so the first sample always computes.
            tone: f32::NAN,
            tone_coeff: 0.0,
        }
    }
}

impl Bd808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        let dt = 1.0 / sample_rate;
        st.timing = Timing::new(sample_rate);
        st.dt = dt;
        st.sample_rate = sample_rate;
        let shaper_tau = C40 * R162 * R163 / (R162 + R163);
        st.shaper_mul = (-dt / shaper_tau).exp();
        st.c38_coeff = 1.0 - (-dt / R156_C38).exp();
        st.c39_step = dt / (R161 * C39);
        st.level_hp_coeff = one_pole_coeff(LEVEL_HP_HZ, sample_rate);
        st.output_hp_coeff = one_pole_coeff(OUTPUT_HP_HZ, sample_rate);
        let rest_r = 1.0 / (1.0 / R161 + 1.0 / R170 + 1.0 / (R165 + R166));
        st.rest_hz = 1.0 / (std::f32::consts::TAU * (rest_r * R167).sqrt() * C41_42);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let freq = p.freq.value_or(ch, DEFAULT_FREQ);
            let decay = p.decay.value_or(ch, DEFAULT_DECAY);
            if freq != cs.network.freq || decay != cs.network.decay {
                cs.network = Network::new(freq, decay, st.rest_hz);
            }
            let tone = p.tone.value_or(ch, DEFAULT_TONE);
            if tone != cs.tone {
                cs.tone = tone;
                cs.tone_coeff = one_pole_coeff(tone_hz(tone), st.sample_rate);
            }
            let controls = Controls {
                network: cs.network,
                tone_coeff: cs.tone_coeff,
            };
            let out = cs.voices.process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                st.timing.choke_samples,
                st,
                &controls,
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

message_handlers!(impl Bd808 {});
