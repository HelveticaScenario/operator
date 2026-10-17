use super::*;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Rs808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud
    /// (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch of the upper resonator in V/Oct, scaling both; stock is
    /// ≈1.79 kHz (default 2.771)
    #[signal(type = pitch, default = 2.771, range = (1.5, 4.0))]
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
struct Rs808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 rim shot.
///
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its rim shot: two resonators near 1.8 kHz and 450 Hz
/// switch a transistor that chops a short envelope, and a resonant high-pass
/// turns the result into a bright, 10 ms crack. **accent**, sampled at each
/// strike, drives the circuit harder.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near
/// **±10 V**.
///
/// ```js
/// $tr808.rs($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.rs", has_init, args(trigger))]
pub struct Rs808 {
    outputs: Rs808Outputs,
    params: Rs808Params,
    state: RimShared,
    channel_state: Box<[Voices<Rim>]>,
}

/// The rim shot's upper resonator as built, Hz.
fn rim_rest_hz() -> f32 {
    RIM_HIGH_T.rest_hz(1.0 / R312)
}

impl Rs808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        st.timing = Timing::new(sample_rate);
        st.substeps = (RIM_RATE / sample_rate).round().max(1.0) as u32;
        let rate = sample_rate * st.substeps as f32;
        st.dt = 1.0 / rate;
        st.c24_charge = 1.0 - (-st.dt / (R107 * C24)).exp();
        st.d63 = d63_table();
        // Backward Euler on the high-pass's capacitors, with
        // C121·v1' = v2/R322 + (E − v1 − v2)/R325 and
        // C122·v2' = (E − v1 − v2)/R325.
        let (a, b) = (st.dt / C121, st.dt / C122);
        let (m11, m12, m21, m22) = (
            1.0 + a / R325,
            a / R325 - a / R322,
            b / R325,
            1.0 + b / R325,
        );
        let det = m11 * m22 - m12 * m21;
        st.crack_inv = [m22 / det, -m12 / det, -m21 / det, m11 / det];
        let (d1, d2) = (a / R325, b / R325);
        st.crack_dir = ((m22 * d1 - m12 * d2) / det, (m11 * d2 - m21 * d1) / det);
        st.couple_coeff = one_pole_coeff(rc_hz(R324 + R326, C125), rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            // `freq` scales both resonators and keeps their stock decays.
            let r = voct_to_hz(p.freq.value_or(ch, 2.771)) / rim_rest_hz();
            let controls = RimControls {
                high: RIM_HIGH_T.tuned(r, T60_TO_RATE / RIM_HIGH_T.decay_rate()),
                low: RIM_LOW_T.tuned(r, T60_TO_RATE / RIM_LOW_T.decay_rate()),
            };
            let out = self.channel_state[ch].process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                st.timing.choke_samples,
                st,
                &controls,
            );
            self.outputs.sample.set(ch, out * RIM_GAIN);
        }
    }
}

message_handlers!(impl Rs808 {});

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cl808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud
    /// (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch of the resonator in V/Oct; stock is ≈2.53 kHz (default 3.273)
    #[signal(type = pitch, default = 3.273, range = (1.5, 4.5))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the ring to fall 60 dB; stock is 0.075 s (default 0.075)
    #[signal(default = 0.075, range = (0.01, 1.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// fade out a still-ringing hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on the
    /// hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cl808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 claves.
///
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its claves: a high-Q bridged-T resonator rings at
/// ≈2.5 kHz. **decay** sets how long it rings, and **accent**, sampled at
/// each strike, drives it harder.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near
/// **±10 V**.
///
/// ```js
/// $tr808.cl($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.cl", has_init, args(trigger))]
pub struct Cl808 {
    outputs: Cl808Outputs,
    params: Cl808Params,
    state: ClavesShared,
    channel_state: Box<[Voices<Claves>]>,
}

impl Cl808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        st.timing = Timing::new(sample_rate);
        st.dt = 1.0 / sample_rate;
        st.couple_coeff = one_pole_coeff(rc_hz(R319 + R326, C125), sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        let rest_hz = CLAVES_T.rest_hz(1.0 / R312);
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let r = voct_to_hz(p.freq.value_or(ch, 3.273)) / rest_hz;
            let decay = p
                .decay
                .value_or(ch, CLAVES_DECAY)
                .clamp(MIN_DECAY, MAX_DECAY);
            let t = CLAVES_T.tuned(r, decay);
            let out = self.channel_state[ch].process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                st.timing.choke_samples,
                st,
                &t,
            );
            self.outputs.sample.set(ch, out * CLAVES_GAIN);
        }
    }
}

message_handlers!(impl Cl808 {});
