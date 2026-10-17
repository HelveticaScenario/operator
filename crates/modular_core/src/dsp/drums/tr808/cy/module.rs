use super::*;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cy808Params {
    /// rising edges strike the cymbal
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse, for a louder hit (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// decay control, 0–5 V: how long the low band rings (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// tone control, 0–5 V: from dark, the high band cut, to bright
    /// (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the envelopes, as
    /// on the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cy808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 cymbal.
///
/// Each rising edge on **trigger** fires the 808's trigger pulse into a
/// circuit model of its cymbal: six square-wave oscillators, filtered into
/// two bands and gated by three envelopes of different lengths. **decay**
/// sets how long the low band rings, **tone** the balance of the bands, and
/// **accent**, sampled at each strike, drives it harder.
///
/// An unaccented hit peaks near **±5 V**.
///
/// ```js
/// $tr808.cy($clock.barTrigger).out()
/// ```
#[module(name = "$tr808.cy", has_init, patch_update, args(trigger))]
pub struct Cy808 {
    outputs: Cy808Outputs,
    params: Cy808Params,
    state: CyState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct CyState {
    shared: CymbalShared,
    filters: MetalFilters,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Cymbal>,
    bank: MetalBank,
}

/// Conductance of a linear pot half at `fraction` of its track `ohms`.
fn pot(ohms: f32, fraction: f32) -> f32 {
    1.0 / (ohms * fraction.clamp(0.0, 1.0) + POT_END)
}

impl Cy808 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.smooth_coeff = 1.0 - (-dt / SMOOTH_TAU).exp();
        sh.env = envelope_network(dt);
        sh.high = high_network(dt);
        sh.mid = mid_network(dt);
        sh.low = low_network(dt);
        sh.tone = tone_network(dt);
        sh.rest = SwingState::rest();
        self.state.filters = MetalFilters::new(sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let (high, low) = cs.bank.step(&st.filters);
            let decay = p.decay.value_or(ch, DEFAULT_DECAY).clamp(0.0, 5.0) / 5.0;
            let tone = p.tone.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let controls = CymbalControls {
                high,
                low,
                decay: 1.0 / (R93 + VR2 * decay),
                tone: [pot(VR4, tone), pot(VR4, 1.0 - tone)],
            };
            let out = cs.voices.process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                st.shared.timing.choke_samples,
                &st.shared,
                &controls,
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

impl crate::types::PatchUpdateHandler for Cy808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.bank.seed(base, ch);
        }
    }
}

message_handlers!(impl Cy808 {});
