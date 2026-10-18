use super::*;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Sd808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud
    /// (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tone control, 0–5 V: crossfades from the low resonator to the high
    /// one (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// snappy control, 0–5 V: how hard each hit charges the noise envelope
    /// (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    snappy: Option<PolySignal>,
    /// pitch of the low resonator in V/Oct, scaling both; stock is ≈250 Hz
    /// (default -0.0678)
    #[signal(type = pitch, default = -0.0678, range = (-1.5, 1.5))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on the
    /// hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Sd808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

#[derive(Clone, Copy)]
struct ChannelState {
    voices: Voices<Voice>,
    noise: WhiteNoise,
    tuning: Tuning,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            voices: Voices::default(),
            noise: WhiteNoise::default(),
            tuning: Tuning {
                // NaN never compares equal, so the first sample always tunes.
                freq: f32::NAN,
                low: LOW,
                high: HIGH,
            },
        }
    }
}

/// TR-808 snare drum.
///
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its snare: two bridged-T resonators near 250 and 500 Hz,
/// blended by **tone**, over a burst of chopped white noise whose strength
/// **snappy** sets. **accent**, sampled at each strike, drives the circuit
/// harder.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near
/// **±10 V**.
///
/// ```js
/// $tr808.sd($clock.beatTrigger, { snappy: 4 }).out()
/// ```
#[module(name = "$tr808.sd", has_init, patch_update, args(trigger))]
pub struct Sd808 {
    outputs: Sd808Outputs,
    params: Sd808Params,
    state: Sd808State,
    channel_state: Box<[ChannelState]>,
}

impl Sd808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        let dt = 1.0 / sample_rate;
        st.dt = dt;
        st.timing = Timing::new(sample_rate);
        st.rest_hz = LOW.rest_hz(1.0 / R196);
        st.low_decay = T60_TO_RATE / LOW.decay_rate();
        st.high_decay = T60_TO_RATE / HIGH.decay_rate();
        st.noise_scale = WhiteNoise::scale(sample_rate);
        let tau = C57 * R189 * R190 / (R189 + R190);
        st.c57_mul = (-dt / tau).exp();
        st.c57_avg = tau / dt * (1.0 - st.c57_mul);
        // A one-pole low-pass `y += c·(x − y)` passes c / (2 − c) of white
        // noise's power; the generator's uniform samples have rms 1/√3.
        st.chop_coeff = 1.0 - (-std::f32::consts::TAU * CHOP_HZ * dt).exp();
        let scale = WhiteNoise::scale(sample_rate);
        st.chop_norm = 3f32.sqrt() / scale * ((2.0 - st.chop_coeff) / st.chop_coeff).sqrt();
        st.snap_hp = Biquad::sallen_key_high(C66, C67, R201, R202, sample_rate);
        st.snap_couple_coeff = one_pole_coeff(rc_hz(R205, C69), sample_rate);
        st.out_lp_coeff = one_pole_coeff(rc_hz(R_LEVEL, C70), sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let freq = p.freq.value_or(ch, DEFAULT_FREQ);
            if freq != cs.tuning.freq {
                cs.tuning = Tuning::new(st, freq);
            }
            let controls = Controls {
                low: cs.tuning.low,
                high: cs.tuning.high,
                tone: p.tone.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0,
                snappy: p.snappy.value_or(ch, DEFAULT_SNAPPY).clamp(0.0, 5.0) / 5.0,
                white: cs.noise.next(st.noise_scale),
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

impl crate::types::PatchUpdateHandler for Sd808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.noise.seed(base, ch);
        }
    }
}

message_handlers!(impl Sd808 {});
