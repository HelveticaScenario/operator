//! `$eq`: serial parametric EQ of trapezoidal state-variable filter
//! sections.

use schemars::JsonSchema;

use crate::dsp::utils::sanitize;
use crate::poly::PolyOutput;

mod live;
mod params;
mod svf;

pub use live::eq_state_builder;

use params::{EqSections, MAX_EQ_SECTIONS, ParametricEqParams, eq_derive_channel_count};
use svf::SectionRuntime;

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ParametricEqOutputs {
    #[output("output", "equalized signal", default)]
    sample: PolyOutput,
}

#[derive(Clone, Copy)]
struct ParametricEqChannel {
    /// Per-section integrator states.
    ic1: [f32; MAX_EQ_SECTIONS],
    ic2: [f32; MAX_EQ_SECTIONS],
    /// In the per-channel form, the coefficients of list `c` live on channel
    /// `c`, and every channel reads its list's from there.
    per: [SectionRuntime; MAX_EQ_SECTIONS],
}

impl Default for ParametricEqChannel {
    fn default() -> Self {
        Self {
            ic1: [0.0; MAX_EQ_SECTIONS],
            ic2: [0.0; MAX_EQ_SECTIONS],
            per: [SectionRuntime::default(); MAX_EQ_SECTIONS],
        }
    }
}

struct ParametricEqState {
    /// Coefficients of the shared section list, used by every channel.
    mono: [SectionRuntime; MAX_EQ_SECTIONS],
    /// The runtimes that ran last; the others hold smoothing from an older
    /// form. None before the first patch update.
    active: Option<ActiveRuntimes>,
}

/// Which runtimes a sections form drives.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveRuntimes {
    /// `state.mono`.
    Shared,
    /// `per` on the first this-many channels.
    PerChannel(usize),
}

impl Default for ParametricEqState {
    fn default() -> Self {
        Self {
            mono: [SectionRuntime::default(); MAX_EQ_SECTIONS],
            active: None,
        }
    }
}

/// Parametric EQ: shape the tone of a sound with up to 8 bands.
///
/// Each band in **sections** has a **type**:
///
/// - `'peak'` (default): boost or cut around a frequency.
/// - `'lowShelf'` / `'highShelf'`: boost or cut everything below / above a
///   frequency.
/// - `'lowpass'` / `'highpass'`: remove everything above / below a frequency.
/// - `'notch'`: remove a narrow band around a frequency.
///
/// and sets:
///
/// - **freq**: where the band sits, like `'1000hz'` or `'c4'`.
/// - **gain**: how much to boost or cut, in dB (-24 to 24). Peaks and shelves
///   only.
/// - **q**: how narrow the band is (0.1 to 20, default 0.707). On shelves and
///   lowpass/highpass, values above 0.707 add a resonant bump at the
///   frequency; on shelves, lower values make a gentler slope.
///
/// Any of them can be modulated. With no sections, the sound passes through
/// unchanged.
///
/// To EQ channels differently, give **sections** one list of bands per
/// channel, like one for left and one for right.
///
/// **adaptiveQ** makes peaks narrower the more you boost or cut them, like
/// many classic analog EQs.
///
/// **ui: true** shows an editor under the call. Drag a band's dot to move it,
/// scroll over it or drag its handles to change q, and double-click to add a
/// band. Changes play right away and are written into your code. Hold
/// Ctrl/Cmd to change only the code, and right-click a control to reset it.
///
/// ```js
/// // Warm up the lows, sweep a dip through the mids, clear out rumble
/// $eq($saw('c2'), {
///   sections: [
///     { type: 'lowShelf', freq: '150hz', gain: 3 },
///     { type: 'peak', freq: $sine('0.5hz').range('400hz', '2000hz'), gain: -6, q: 2 },
///     { type: 'highpass', freq: '40hz' },
///   ],
///   adaptiveQ: true,
/// })
///
/// // A darker left channel, a brighter right one
/// $eq($saw(['c2', 'g2']), {
///   sections: [
///     [{ type: 'highShelf', freq: '3000hz', gain: -6 }],
///     [{ type: 'highShelf', freq: '3000hz', gain: 4 }],
///   ],
/// })
///
/// // Start empty and build the EQ in the editor
/// $eq($saw('c2'), { ui: true })
/// ```
#[module(name = "$eq", channels_derive = eq_derive_channel_count, patch_update, stateful, args(input))]
pub struct ParametricEq {
    outputs: ParametricEqOutputs,
    state: ParametricEqState,
    channel_state: Box<[ParametricEqChannel]>,
    params: ParametricEqParams,
}

impl ParametricEq {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let adaptive_q = self.params.adaptive_q;
        match &self.params.sections {
            EqSections::Shared(list) => {
                let n = list.len().min(MAX_EQ_SECTIONS);
                for (rt, sec) in self.state.mono.iter_mut().zip(&list[..n]) {
                    rt.refresh(sec, sample_rate, adaptive_q);
                }
                for ch in 0..channels {
                    let cs = &mut self.channel_state[ch];
                    let mut x = sanitize(self.params.input.get_value(ch));
                    for s in 0..n {
                        x = svf::process(
                            &self.state.mono[s].coeffs,
                            &mut cs.ic1[s],
                            &mut cs.ic2[s],
                            x,
                        );
                    }
                    self.outputs.sample.set(ch, x);
                }
            }
            EqSections::PerChannel(lists) => {
                // List `l` is computed once, on channel `l`'s runtimes; there
                // are never fewer channels than lists.
                let num_lists = lists.len().min(channels);
                for (l, list) in lists.iter().take(num_lists).enumerate() {
                    let n = list.len().min(MAX_EQ_SECTIONS);
                    let per = &mut self.channel_state[l].per;
                    for (rt, sec) in per.iter_mut().zip(&list[..n]) {
                        rt.refresh(sec, sample_rate, adaptive_q);
                    }
                }
                for ch in 0..channels {
                    let l = ch % num_lists;
                    let n = lists[l].len().min(MAX_EQ_SECTIONS);
                    let mut x = sanitize(self.params.input.get_value(ch));
                    for s in 0..n {
                        let c = self.channel_state[l].per[s].coeffs;
                        let cs = &mut self.channel_state[ch];
                        x = svf::process(&c, &mut cs.ic1[s], &mut cs.ic2[s], x);
                    }
                    self.outputs.sample.set(ch, x);
                }
            }
        }
    }
}

impl crate::types::PatchUpdateHandler for ParametricEq {
    /// Section types, section order, the sections form, and adaptive Q
    /// change only on patch updates and are invisible to the per-sample
    /// change gate, so every section recomputes its coefficients on the next
    /// sample.
    ///
    /// Runtimes coming back into use after another form ran hold stale
    /// smoothing, so they start fresh and snap to the new values.
    fn on_patch_update(&mut self) {
        let now = match &self.params.sections {
            EqSections::Shared(_) => ActiveRuntimes::Shared,
            EqSections::PerChannel(lists) => {
                ActiveRuntimes::PerChannel(lists.len().min(self.channel_count()))
            }
        };
        let fresh_lists = match (self.state.active, now) {
            (Some(ActiveRuntimes::Shared), ActiveRuntimes::Shared) => 0..0,
            (_, ActiveRuntimes::Shared) => {
                self.state.mono = [SectionRuntime::default(); MAX_EQ_SECTIONS];
                0..0
            }
            (Some(ActiveRuntimes::PerChannel(was)), ActiveRuntimes::PerChannel(n)) => was.min(n)..n,
            (_, ActiveRuntimes::PerChannel(n)) => 0..n,
        };
        for cs in &mut self.channel_state[fresh_lists] {
            cs.per = [SectionRuntime::default(); MAX_EQ_SECTIONS];
        }
        self.state.active = Some(now);

        for rt in self.state.mono.iter_mut() {
            rt.invalidate();
        }
        for cs in self.channel_state.iter_mut() {
            for rt in cs.per.iter_mut() {
                rt.invalidate();
            }
        }
    }
}

impl crate::types::StatefulModule for ParametricEq {
    /// Publish each list's running section values for the editor panel.
    fn write_module_state(&self, out: &mut dyn crate::module_state::ModuleLiveState) {
        let Some(out) = out.as_any_mut().downcast_mut::<live::EqLiveState>() else {
            return;
        };
        crate::module_state::ModuleLiveState::reset(out);
        match &self.params.sections {
            EqSections::Shared(list) => {
                let n = list.len().min(MAX_EQ_SECTIONS);
                out.write_list(0, self.state.mono[..n].iter().map(|rt| rt.live_values()));
            }
            EqSections::PerChannel(lists) => {
                let num_lists = lists.len().min(self.channel_count());
                for (l, list) in lists.iter().take(num_lists).enumerate() {
                    let n = list.len().min(MAX_EQ_SECTIONS);
                    let per = &self.channel_state[l].per[..n];
                    out.write_list(l, per.iter().map(|rt| rt.live_values()));
                }
            }
        }
    }
}

message_handlers!(impl ParametricEq {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};
    use params::{DEFAULT_Q, EqSection, EqSectionType};

    fn make(sections: Vec<EqSection>) -> ParametricEq {
        make_with(EqSections::Shared(sections), 1)
    }

    fn make_with(sections: EqSections, channels: usize) -> ParametricEq {
        let mut outputs = ParametricEqOutputs::default();
        outputs.set_all_channels(channels);
        ParametricEq {
            params: ParametricEqParams {
                input: PolySignal::mono(Signal::Volts(0.0)),
                sections,
                adaptive_q: false,
                ui: false,
            },
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
            state: ParametricEqState::default(),
            channel_state: vec![ParametricEqChannel::default(); channels].into_boxed_slice(),
        }
    }

    fn section(kind: EqSectionType, gain_db: f32, q: f32) -> EqSection {
        EqSection {
            r#type: kind,
            freq: PolySignal::mono(Signal::Volts(0.0)).into(),
            gain: Some(PolySignal::mono(Signal::Volts(gain_db)).into()),
            q: Some(PolySignal::mono(Signal::Volts(q)).into()),
        }
    }

    /// RMS of the module output for a unit sine at `freq_hz`, skipping the
    /// first 1000 samples of smoothing/filter transient.
    fn sine_rms(m: &mut ParametricEq, freq_hz: f32, n: usize) -> f32 {
        let mut acc = 0.0f64;
        let mut count = 0usize;
        for i in 0..n {
            let x = (std::f32::consts::TAU * freq_hz * i as f32 / 48000.0).sin();
            m.params.input = PolySignal::mono(Signal::Volts(x));
            m.update(48000.0);
            if i >= 1000 {
                let y = m.outputs.sample.get(0) as f64;
                acc += y * y;
                count += 1;
            }
        }
        (acc / count as f64).sqrt() as f32
    }

    const C4_HZ: f32 = 261.63;
    const UNIT_SINE_RMS: f32 = std::f32::consts::FRAC_1_SQRT_2;

    #[test]
    fn peak_boost_raises_center_leaves_distant_band() {
        let mut m = make(vec![section(EqSectionType::Peak, 12.0, 1.0)]);
        let center = sine_rms(&mut m, C4_HZ, 20_000);
        assert!(
            center > 2.5 * UNIT_SINE_RMS,
            "+12 dB peak should raise center RMS, got {center}"
        );
        let mut m = make(vec![section(EqSectionType::Peak, 12.0, 1.0)]);
        let distant = sine_rms(&mut m, 8000.0, 20_000);
        assert!(
            (distant - UNIT_SINE_RMS).abs() < 0.2 * UNIT_SINE_RMS,
            "distant band should be near unity, got {distant}"
        );
    }

    #[test]
    fn notch_kills_center() {
        let mut m = make(vec![section(EqSectionType::Notch, 0.0, 4.0)]);
        let center = sine_rms(&mut m, C4_HZ, 20_000);
        assert!(
            center < 0.25 * UNIT_SINE_RMS,
            "notch should attenuate center, got {center}"
        );
        let mut m = make(vec![section(EqSectionType::Notch, 0.0, 4.0)]);
        let distant = sine_rms(&mut m, 8000.0, 20_000);
        assert!(
            (distant - UNIT_SINE_RMS).abs() < 0.2 * UNIT_SINE_RMS,
            "distant band should be near unity, got {distant}"
        );
    }

    #[test]
    fn zero_gain_peak_is_identity() {
        let mut m = make(vec![section(EqSectionType::Peak, 0.0, 1.0)]);
        let rms = sine_rms(&mut m, C4_HZ, 20_000);
        assert!(
            (rms - UNIT_SINE_RMS).abs() < 0.02 * UNIT_SINE_RMS,
            "0 dB peak should pass through, got {rms}"
        );
    }

    #[test]
    fn adaptive_q_narrows_boosted_peak() {
        let rms_pair = |adaptive: bool| {
            let mut m = make(vec![section(EqSectionType::Peak, 12.0, 1.0)]);
            m.params.adaptive_q = adaptive;
            let center = sine_rms(&mut m, C4_HZ, 20_000);
            let mut m = make(vec![section(EqSectionType::Peak, 12.0, 1.0)]);
            m.params.adaptive_q = adaptive;
            let octave_up = sine_rms(&mut m, 2.0 * C4_HZ, 20_000);
            (center, octave_up)
        };
        let (fixed_center, fixed_skirt) = rms_pair(false);
        let (adaptive_center, adaptive_skirt) = rms_pair(true);
        assert!(
            (adaptive_center - fixed_center).abs() < 0.02 * fixed_center,
            "adaptive Q keeps the peak gain, got {adaptive_center} vs {fixed_center}"
        );
        assert!(
            adaptive_skirt < 0.8 * fixed_skirt,
            "adaptive Q narrows the bell, skirt {adaptive_skirt} vs {fixed_skirt}"
        );
    }

    #[test]
    fn adaptive_q_leaves_shelves_unchanged() {
        let rms_at_f0 = |adaptive: bool| {
            let mut m = make(vec![section(EqSectionType::LowShelf, 12.0, DEFAULT_Q)]);
            m.params.adaptive_q = adaptive;
            sine_rms(&mut m, C4_HZ, 20_000)
        };
        assert!((rms_at_f0(true) - rms_at_f0(false)).abs() < 1e-4);
    }

    /// Peak and trough of a +12 dB low shelf's response across ±2 octaves.
    fn shelf_extremes(q: f32) -> (f32, f32) {
        let (mut hi, mut lo) = (0.0f32, f32::MAX);
        for k in -6..=6 {
            let freq = C4_HZ * 2f32.powf(k as f32 / 3.0);
            let mut m = make(vec![section(EqSectionType::LowShelf, 12.0, q)]);
            let rms = sine_rms(&mut m, freq, 20_000);
            hi = hi.max(rms);
            lo = lo.min(rms);
        }
        (hi, lo)
    }

    #[test]
    fn shelf_overshoots_only_above_0_707() {
        let plateau = UNIT_SINE_RMS * 10f32.powf(12.0 / 20.0);
        let (hi, lo) = shelf_extremes(DEFAULT_Q);
        assert!(
            hi <= plateau * 1.01 && lo >= UNIT_SINE_RMS * 0.99,
            "a 0.707 shelf stays within [0, 12] dB: {lo}..{hi}"
        );
        let (hi, lo) = shelf_extremes(4.0);
        assert!(
            hi > plateau * 1.1,
            "a q 4 shelf bumps above its plateau: {hi}"
        );
        assert!(lo < UNIT_SINE_RMS * 0.9, "and dips below unity: {lo}");
    }

    #[test]
    fn patch_update_recomputes_when_only_type_changes() {
        use crate::types::PatchUpdateHandler;
        let mut m = make(vec![section(EqSectionType::Peak, 0.0, 4.0)]);
        sine_rms(&mut m, C4_HZ, 2000);
        let EqSections::Shared(list) = &mut m.params.sections else {
            unreachable!()
        };
        list[0].r#type = EqSectionType::Notch;
        m.on_patch_update();
        let center = sine_rms(&mut m, C4_HZ, 20_000);
        assert!(
            center < 0.25 * UNIT_SINE_RMS,
            "notch must take effect after the patch update, got {center}"
        );
    }

    #[test]
    fn per_channel_lists_eq_each_channel_independently() {
        // Four channels over two lists: channels 2 and 3 reuse lists 0 and 1.
        let sections = EqSections::PerChannel(vec![
            vec![section(EqSectionType::Notch, 0.0, 4.0)],
            vec![section(EqSectionType::Peak, 0.0, 1.0)],
        ]);
        let mut m = make_with(sections, 4);
        let mut acc = [0.0f64; 4];
        for i in 0..20_000 {
            let x = (std::f32::consts::TAU * C4_HZ * i as f32 / 48000.0).sin();
            m.params.input = PolySignal::mono(Signal::Volts(x));
            m.update(48000.0);
            if i >= 1000 {
                for (ch, a) in acc.iter_mut().enumerate() {
                    *a += (m.outputs.sample.get(ch) as f64).powi(2);
                }
            }
        }
        let rms = acc.map(|a| (a / 19_000.0).sqrt() as f32);
        for ch in [0, 2] {
            assert!(
                rms[ch] < 0.25 * UNIT_SINE_RMS,
                "channel {ch} notched: {}",
                rms[ch]
            );
        }
        for ch in [1, 3] {
            assert!(
                (rms[ch] - UNIT_SINE_RMS).abs() < 0.02 * UNIT_SINE_RMS,
                "channel {ch} passes: {}",
                rms[ch]
            );
        }
    }

    #[test]
    fn publishes_running_values_per_list() {
        use crate::types::StatefulModule;
        let mut m = make_with(
            EqSections::PerChannel(vec![
                vec![section(EqSectionType::Peak, 6.0, 2.0)],
                vec![
                    section(EqSectionType::Peak, -30.0, 50.0),
                    section(EqSectionType::Notch, 0.0, 1.0),
                ],
            ]),
            2,
        );
        for _ in 0..20_000 {
            m.update(48000.0);
        }
        let params = serde_json::json!({ "ui": true, "sections": [[{}], [{}, {}]] });
        let (mut live, meta) = live::eq_state_builder(&params).unwrap();
        m.write_module_state(&mut *live);
        let json = meta.build_json(&*live);
        let lists = json["eqLive"].as_array().unwrap();
        let close = |v: &serde_json::Value, want: f64| (v.as_f64().unwrap() - want).abs() < 1e-3;
        let first = &lists[0][0];
        assert!(
            close(&first[0], 0.0) && close(&first[1], 6.0) && close(&first[2], 2.0),
            "{first}"
        );
        // Gain and q are reported after clamping.
        let second = &lists[1][0];
        assert!(
            close(&second[1], -24.0) && close(&second[2], 20.0),
            "{second}"
        );
        assert_eq!(lists[1].as_array().unwrap().len(), 2);
    }

    #[test]
    fn settled_section_takes_a_changed_value_after_patch_update() {
        use crate::types::PatchUpdateHandler;
        let mut m = make(vec![section(EqSectionType::Peak, 0.0, 1.0)]);
        // Long enough for the smoothers to reach their targets and settle.
        sine_rms(&mut m, C4_HZ, 20_000);
        let EqSections::Shared(list) = &mut m.params.sections else {
            unreachable!()
        };
        list[0].gain = Some(PolySignal::mono(Signal::Volts(12.0)).into());
        m.on_patch_update();
        let center = sine_rms(&mut m, C4_HZ, 20_000);
        assert!(
            center > 2.5 * UNIT_SINE_RMS,
            "the new +12 dB gain must apply, got {center}"
        );
    }

    #[test]
    fn non_finite_input_never_corrupts_the_chain() {
        let mut m = make(vec![
            section(EqSectionType::Peak, 12.0, 4.0),
            section(EqSectionType::LowShelf, -6.0, 0.7),
        ]);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            m.params.input = PolySignal::mono(Signal::Volts(bad));
            m.update(48000.0);
            assert!(m.outputs.sample.get(0).is_finite());
        }
        let rms = sine_rms(&mut m, 8000.0, 20_000);
        assert!(
            rms.is_finite() && rms > 0.1,
            "the chain recovers, got {rms}"
        );
    }

    #[test]
    fn returning_to_a_form_does_not_glide_from_its_old_values() {
        use crate::types::PatchUpdateHandler;
        let band = |volts: f32| {
            let mut s = section(EqSectionType::Peak, 6.0, 1.0);
            s.freq = PolySignal::mono(Signal::Volts(volts)).into();
            s
        };
        let mut m = make_with(EqSections::Shared(vec![band(-1.0)]), 2);
        m.on_patch_update();
        sine_rms(&mut m, C4_HZ, 2000);
        m.params.sections = EqSections::PerChannel(vec![vec![band(-1.0)], vec![band(-1.0)]]);
        m.on_patch_update();
        sine_rms(&mut m, C4_HZ, 2000);
        m.params.sections = EqSections::Shared(vec![band(4.0)]);
        m.on_patch_update();
        m.update(48000.0);
        let freq = m.state.mono[0].live_values()[0];
        assert!(
            (freq - 4.0).abs() < 1e-3,
            "the shared runtime starts at the new freq, got {freq} V"
        );
    }

    #[test]
    fn eight_sections_stay_finite() {
        let sections = vec![
            section(EqSectionType::LowShelf, 6.0, 0.7),
            section(EqSectionType::Peak, -12.0, 2.0),
            section(EqSectionType::Peak, 12.0, 8.0),
            section(EqSectionType::HighShelf, -6.0, 0.7),
            section(EqSectionType::Lowpass, 0.0, 2.0),
            section(EqSectionType::Highpass, 0.0, 2.0),
            section(EqSectionType::Notch, 0.0, 4.0),
            section(EqSectionType::Peak, 24.0, 20.0),
        ];
        let mut m = make(sections);
        for i in 0..8000 {
            let x = (std::f32::consts::TAU * 440.0 * i as f32 / 48000.0).sin();
            m.params.input = PolySignal::mono(Signal::Volts(x));
            m.update(48000.0);
            let y = m.outputs.sample.get(0);
            assert!(y.is_finite(), "8-section chain should stay finite, got {y}");
        }
    }
}
