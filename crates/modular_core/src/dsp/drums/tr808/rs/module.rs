use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Cached, Input, Live, each_slot};
use crate::dsp::drums::tr808::circuit::PULSE_SECS;
use crate::dsp::drums::tr808::levels;
use crate::dsp::drums::voice::{Timing, rails};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Rs808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the hit: up to twice
    /// as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch of the upper tone in V/Oct, the lower one following; stock is
    /// ≈1.79 kHz (default 2.771)
    #[signal(type = pitch, default = 2.771, range = (1.5, 4.0))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a hit still sounding over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise each hit builds on the last, as on
    /// the 808 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Rs808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 rim shot.
///
/// A circuit model of the 808's rim shot: a bright, woody crack, over in about
/// 10 ms, built from two tones near 1.8 kHz and 450 Hz. **freq** sets its pitch
/// and **accent** how hard it hits.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr808.rimShot($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.rimShot", has_init, args(trigger), block)]
pub struct Rs808 {
    outputs: Rs808Outputs,
    params: Rs808Params,
    state: RimShared,
    channel_state: Box<[RimChannel]>,
}

/// A rim shot channel: its circuits, and its resonators tuned to its
/// `freq` with the output gain levelling it.
#[derive(Clone, Copy, Default)]
struct RimChannel {
    voices: Voices<Rim>,
    controls: Cached<f32, (RimControls, f32)>,
}

/// A claves channel: its circuits, and its resonator tuned to its `freq`
/// and `decay` with the output gain levelling `freq`.
#[derive(Clone, Copy, Default)]
struct ClavesChannel {
    voices: Voices<Claves>,
    t: Cached<(f32, f32), (BridgedT, f32)>,
}

fn rim_rest_hz() -> f32 {
    RIM_HIGH_T.rest_hz(1.0 / R312)
}

impl Rs808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        st.timing = Timing::new(PULSE_SECS, sample_rate);
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
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Rs808Params::ACCENT_DEFAULT),
            Live::or(&p.freq, Rs808Params::FREQ_DEFAULT),
        ];
        let out = &mut self.outputs.sample;
        rim_tick(
            &self.state,
            &mut self.channel_state[..channels],
            p.restart,
            inputs,
            |ch, v| out.set(ch, v),
        );
    }

    fn process_block(
        &mut self,
        outputs: &mut Rs808BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Rs808ParamsBlockInputs {
            trigger,
            accent,
            freq,
        } = p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent, freq],
            |inputs, mut slot| {
                rim_tick(&self.state, channel_state, p.restart, inputs, |ch, v| {
                    slot.set(ch, v)
                })
            },
        );
        Some(())
    }
}

/// Runs one sample of every rim shot channel on its `[trigger, accent,
/// freq]`, handing `out` each one's output.
#[inline(always)]
fn rim_tick<I: Input>(
    st: &RimShared,
    channels: &mut [RimChannel],
    restart: bool,
    [trigger, accent, freq]: [I; 3],
    mut out: impl FnMut(usize, f32),
) {
    for (ch, cs) in channels.iter_mut().enumerate() {
        // `freq` scales both resonators and keeps their stock decays.
        let (controls, level) = cs.controls.get(freq.at(ch), |freq| {
            let r = voct_to_hz(freq) / rim_rest_hz();
            let controls = RimControls {
                high: RIM_HIGH_T.tuned(r, T60_TO_RATE / RIM_HIGH_T.decay_rate()),
                low: RIM_LOW_T.tuned(r, T60_TO_RATE / RIM_LOW_T.decay_rate()),
            };
            (controls, levels::RIM_SHOT.gain(freq))
        });
        let v = cs.voices.process(
            trigger.at(ch),
            accent,
            ch,
            restart,
            st.timing.fade_samples,
            st,
            &controls,
        );
        out(ch, rails(v * RIM_GAIN * level));
    }
}

message_handlers!(impl Rs808 {});

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cl808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the hit: up to twice
    /// as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch in V/Oct; stock is ≈2.53 kHz (default 3.273)
    #[signal(type = pitch, default = 3.273, range = (1.5, 4.5))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// decay control, 0–5 V: how long the claves ring, from ≈0.025 s to
    /// ≈0.23 s (default 2.5, ≈0.075 s as on the 808)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// fade out a hit still sounding over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise each hit builds on the last, as on
    /// the 808 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cl808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 claves.
///
/// A circuit model of the 808's claves: a short, pure wooden click ringing at
/// ≈2.5 kHz. **decay** sets how long it rings, **freq** its pitch, and
/// **accent** how hard it hits.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr808.claves($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.claves", has_init, args(trigger), block)]
pub struct Cl808 {
    outputs: Cl808Outputs,
    params: Cl808Params,
    state: ClavesShared,
    channel_state: Box<[ClavesChannel]>,
}

impl Cl808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        st.timing = Timing::new(PULSE_SECS, sample_rate);
        st.dt = 1.0 / sample_rate;
        st.couple_coeff = one_pole_coeff(rc_hz(R319 + R326, C125), sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Cl808Params::ACCENT_DEFAULT),
            Live::or(&p.freq, Cl808Params::FREQ_DEFAULT),
            Live::or(&p.decay, Cl808Params::DECAY_DEFAULT),
        ];
        let out = &mut self.outputs.sample;
        claves_tick(
            &self.state,
            &mut self.channel_state[..channels],
            p.restart,
            inputs,
            |ch, v| out.set(ch, v),
        );
    }

    fn process_block(
        &mut self,
        outputs: &mut Cl808BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Cl808ParamsBlockInputs {
            trigger,
            accent,
            freq,
            decay,
        } = p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent, freq, decay],
            |inputs, mut slot| {
                claves_tick(&self.state, channel_state, p.restart, inputs, |ch, v| {
                    slot.set(ch, v)
                })
            },
        );
        Some(())
    }
}

/// Runs one sample of every claves channel on its `[trigger, accent, freq,
/// decay]`, handing `out` each one's output.
#[inline(always)]
fn claves_tick<I: Input>(
    st: &ClavesShared,
    channels: &mut [ClavesChannel],
    restart: bool,
    [trigger, accent, freq, decay]: [I; 4],
    mut out: impl FnMut(usize, f32),
) {
    let rest_hz = CLAVES_T.rest_hz(1.0 / R312);
    for (ch, cs) in channels.iter_mut().enumerate() {
        let (t, level) = cs.t.get((freq.at(ch), decay.at(ch)), |(freq, decay)| {
            let r = voct_to_hz(freq) / rest_hz;
            let t = CLAVES_T.tuned(r, decay_around(decay, CLAVES_DECAY));
            (t, levels::CLAVES.gain(freq))
        });
        let v = cs.voices.process(
            trigger.at(ch),
            accent,
            ch,
            restart,
            st.timing.fade_samples,
            st,
            &t,
        );
        out(ch, rails(v * CLAVES_GAIN * level));
    }
}

message_handlers!(impl Cl808 {});
