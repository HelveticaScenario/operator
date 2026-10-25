//! `$reverb`: Beads' reverb and limiters at the engine rate.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::{MonoSignal, MonoSignalExt, PolyOutput, PolySignal};

use super::chain::{BlockClock, POT_LP};
use super::primitives::{Frame, Limiter};
use super::reverb::{Reverb, ReverbSettings};
use super::{FULL_SCALE_VOLTS, fold_input, unit};
use crate::dsp::utils::sanitize;

/// Hardware rate the reverb's delays are tuned to at `size` 0.
const HARDWARE_RATE: f32 = 48000.0;
/// Delay stretch at `size` 5.
const MAX_SIZE: f32 = 2.0;

/// Beads' limiters and reverb, with the control-block clock.
#[derive(Default)]
pub struct OutputStage {
    clock: BlockClock,
    /// The stage's rate over the hardware's.
    scale: f32,
    reverb: Reverb,
    reverb_settings: ReverbSettings,
    pre_reverb_limiter: Limiter,
    output_limiter: Limiter,
}

impl OutputStage {
    /// Allocate for rates up to `max_scale` times the hardware's. Main
    /// thread only.
    pub fn allocate(&mut self, max_scale: f32) {
        super::primitives::warm_tables();
        self.reverb.allocate(max_scale);
    }

    /// Run at `sample_rate`, `scale` times the hardware's rate. The reverb
    /// tank carries over while the scale stays the same.
    pub fn configure(&mut self, sample_rate: f32, scale: f32) {
        self.scale = scale;
        self.clock.configure(sample_rate);
        self.reverb.set_sample_rate(sample_rate, scale);
        self.pre_reverb_limiter.init(1.0 / (sample_rate * 4.0));
        self.output_limiter.init(1.0 / (sample_rate * 6.0));
    }

    /// Wipe the reverb tank, for a fresh module.
    pub fn clear_reverb(&mut self) {
        self.reverb.clear();
    }

    /// Advance the control-block clock by one sample at the stage's rate;
    /// true when a block starts at this sample.
    #[inline]
    pub fn tick_block(&mut self) -> bool {
        self.clock.tick()
    }

    /// Set the block's reverb from the smoothed `reverb` control, which
    /// raises the send and the decay together, with its delays stretched by
    /// `size` (from 1). `darkness` lowers the loop's cutoff exponentially:
    /// 0 is the hardware's bright setting and 1 its dark one, and each further
    /// step lowers the cutoff by the same ratio again.
    pub fn start_block(&mut self, reverb: f32, size: f32, darkness: f32) -> ReverbSettings {
        let block = self.clock.size();
        let r = reverb;
        let time = r * r * r;
        let d = darkness.clamp(0.0, 1.0);
        let blend = |bright: f32, dark: f32| bright * (1.0 - d) + dark * d;
        // The log of the hardware's one-pole feedback sets the cutoff, and
        // the stretch spreads each hardware sample over several here.
        let bright = (1.0 - 0.6f32 as f64).ln();
        let dark = (1.0 - 0.4f32 as f64).ln();
        let log_pole = bright * (dark / bright).powf(darkness as f64);
        let stretch = (self.scale * size) as f64;
        self.reverb_settings = ReverbSettings {
            mix: r * r * 0.9,
            time: blend(0.3 + 0.6 * time, 0.45 + 0.5 * time),
            lp: (1.0 - (log_pole / stretch).exp()) as f32,
            size,
        };
        self.reverb.start_block(&self.reverb_settings, block);
        self.reverb_settings
    }

    /// Run the mixed signal through the rest of the chain.
    pub fn finish(&mut self, mut out: Frame) -> Frame {
        out = self.pre_reverb_limiter.process(out);
        out = self.reverb.process(out, &self.reverb_settings);
        self.output_limiter.process(out)
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct BeadsReverbParams {
    /// audio input; even channels feed the left side and odd channels the
    /// right
    input: PolySignal,
    /// how much reverb, and how long it rings (0–5, default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    amount: Option<MonoSignal>,
    /// room size; moving it while the reverb rings bends the tail's pitch
    /// (0–5, default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    size: Option<MonoSignal>,
    /// how quickly the tail darkens, from bright at 0 to muffled at 5
    /// (default 1)
    #[signal(default = 1.0, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<MonoSignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BeadsReverbOutputs {
    #[output("output", "stereo output (ch0=left, ch1=right)", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// The output stage and its smoothed controls, each from 0 to 1.
#[derive(Default)]
struct BeadsReverbState {
    stage: OutputStage,
    reverb: f32,
    size: f32,
    tone: f32,
    /// Set from the first block on; before it, size and tone start at
    /// their targets.
    started: bool,
}

/// Reverb from Mutable Instruments Beads, with a stereo output.
///
/// `amount` sets both how much reverb you hear and how long it rings; at 0
/// the input passes through untouched. `size` makes the space bigger and
/// `tone` makes the tail darken faster as it fades. Beads' own settings are
/// `size` 0 and `tone` 1 for digital, 0 and 2 for clean tape, 2.5 and 2 for
/// vintage, and 5 and 2 for dirty tape. The input passes through clean, so
/// put `$granular` or `$wowFlutter` before it for their colour. Peaks above
/// ±5 V are limited.
///
/// ```js
/// $reverb($saw('c3'), 4, { size: 5, tone: 2 }).out()
/// ```
#[module(name = "$reverb", channels = 2, has_init, args(input, amount))]
pub struct BeadsReverb {
    outputs: BeadsReverbOutputs,
    state: BeadsReverbState,
    params: BeadsReverbParams,
}

impl BeadsReverb {
    /// Allocate buffers and set the rate. Main thread.
    fn init(&mut self, sample_rate: f32) {
        let stage = &mut self.state.stage;
        let scale = sample_rate / HARDWARE_RATE;
        stage.allocate(scale * MAX_SIZE);
        stage.configure(sample_rate, scale);
        stage.clear_reverb();
    }

    fn update(&mut self, _sample_rate: f32) {
        let input = fold_input(&self.params.input);
        let params = &self.params;
        let BeadsReverbState {
            stage,
            reverb,
            size,
            tone,
            started,
        } = &mut self.state;
        if stage.tick_block() {
            let control = |s: &Option<MonoSignal>, default| unit(sanitize(s.value_or(default)));
            let (size_target, tone_target) =
                (control(&params.size, 0.0), control(&params.tone, 1.0));
            if !*started {
                (*size, *tone, *started) = (size_target, tone_target, true);
            }
            *reverb += POT_LP * (control(&params.amount, 2.5) - *reverb);
            *size += POT_LP * (size_target - *size);
            *tone += POT_LP * (tone_target - *tone);
            // Beads' bright loop is `tone` 1 and its dark loop `tone` 2.
            let darkness = *tone * FULL_SCALE_VOLTS - 1.0;
            stage.start_block(*reverb, 1.0 + (MAX_SIZE - 1.0) * *size, darkness);
        }
        let out = stage.finish(input);
        self.outputs.sample.set(0, out.l * FULL_SCALE_VOLTS);
        self.outputs.sample.set(1, out.r * FULL_SCALE_VOLTS);
    }
}

message_handlers!(impl BeadsReverb {});

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48000.0;

    /// `size` and `tone` of Beads' digital, clean tape, vintage and dirty
    /// tape settings.
    pub const DIGITAL: (f32, f32) = (0.0, 1.0);
    pub const CLEAN_TAPE: (f32, f32) = (0.0, 2.0);
    const VINTAGE: (f32, f32) = (2.5, 2.0);
    const DIRTY_TAPE: (f32, f32) = (5.0, 2.0);
    const SETTINGS: [(f32, f32); 4] = [DIGITAL, CLEAN_TAPE, VINTAGE, DIRTY_TAPE];

    fn volts(v: f32) -> Option<MonoSignal> {
        Some(MonoSignal::from_poly(PolySignal::mono(Signal::Volts(v))))
    }

    pub fn make(setting: (f32, f32), reverb: f32) -> BeadsReverb {
        make_at(setting, reverb, SR)
    }

    fn make_at((size, tone): (f32, f32), reverb: f32, sample_rate: f32) -> BeadsReverb {
        let mut outputs = BeadsReverbOutputs::default();
        outputs.set_all_channels(2);
        let mut m = BeadsReverb {
            outputs,
            state: BeadsReverbState::default(),
            params: BeadsReverbParams {
                input: PolySignal::mono(Signal::Volts(0.0)),
                amount: volts(reverb),
                size: volts(size),
                tone: volts(tone),
            },
            _channel_count: 2,
            _block_index: Default::default(),
        };
        m.init(sample_rate);
        m
    }

    pub fn step(m: &mut BeadsReverb, input: f32) -> (f32, f32) {
        m.params.input = PolySignal::mono(Signal::Volts(input));
        m.update(SR);
        (m.outputs.sample.get(0), m.outputs.sample.get(1))
    }

    /// Energy of the left output in successive 50 ms windows after a 10 ms
    /// burst at `sample_rate`.
    fn tail_envelope(setting: (f32, f32), sample_rate: f32) -> Vec<f32> {
        let mut m = make_at(setting, 4.0, sample_rate);
        let window = (sample_rate * 0.05) as usize;
        let burst = (sample_rate * 0.01) as usize;
        let mut envelope = vec![0.0; 20];
        for n in 0..burst + window * envelope.len() {
            let x = if n < burst {
                (std::f32::consts::TAU * 330.0 * n as f32 / sample_rate).sin() * 2.0
            } else {
                0.0
            };
            let l = step(&mut m, x).0;
            if n >= burst {
                envelope[(n - burst) / window] += l * l / window as f32;
            }
        }
        envelope
    }

    fn late_energy(setting: (f32, f32)) -> f32 {
        tail_envelope(setting, SR)[10..].iter().sum()
    }

    #[test]
    fn reverb_keeps_its_timing_at_any_engine_rate() {
        for setting in SETTINGS {
            let reference = tail_envelope(setting, SR);
            for sample_rate in [44100.0, 96000.0] {
                let envelope = tail_envelope(setting, sample_rate);
                for (w, (a, b)) in reference.iter().zip(&envelope).enumerate() {
                    let db = 10.0 * (b / a).log10();
                    assert!(
                        db.abs() < 1.5,
                        "{setting:?} at {sample_rate} Hz, window {w}: {db} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn larger_sizes_ring_longer() {
        assert!(late_energy(VINTAGE) > late_energy(CLEAN_TAPE));
        assert!(late_energy(DIRTY_TAPE) > late_energy(VINTAGE));
    }

    #[test]
    fn darker_tones_ring_longer() {
        assert!(late_energy(CLEAN_TAPE) > late_energy(DIGITAL));
        assert!(late_energy((0.0, 1.5)) > late_energy(DIGITAL));
        assert!(late_energy(CLEAN_TAPE) > late_energy((0.0, 1.5)));
    }

    /// Brightness of the tail after a 10 ms noise burst: the energy of its
    /// first difference over its own energy, in dB.
    fn tail_brightness(tone: f32) -> f32 {
        let mut m = make((0.0, tone), 4.0);
        let mut rng = crate::dsp::utils::rng::LcgRng::default();
        rng.seed(3, 0);
        for _ in 0..480 {
            step(&mut m, (rng.next_unit() - 0.5) * 4.0);
        }
        let (mut energy, mut difference, mut previous) = (0.0, 0.0, 0.0);
        for _ in 0..24_000 {
            let l = step(&mut m, 0.0).0;
            energy += l * l;
            difference += (l - previous) * (l - previous);
            previous = l;
        }
        10.0 * (difference / energy).log10()
    }

    #[test]
    fn tone_darkens_the_tail_across_its_range() {
        let brightness = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0].map(tail_brightness);
        for pair in brightness.windows(2) {
            assert!(pair[1] < pair[0] - 1.0, "{brightness:?}");
        }
        assert!(brightness[0] - brightness[5] > 6.0, "{brightness:?}");
    }

    #[test]
    fn silence_stays_silent_at_every_setting() {
        for setting in SETTINGS {
            let mut m = make(setting, 5.0);
            for _ in 0..20_000 {
                let (l, r) = step(&mut m, 0.0);
                assert_eq!((l, r), (0.0, 0.0), "{setting:?}");
            }
        }
    }

    #[test]
    fn passes_the_input_at_unity_without_reverb() {
        for setting in SETTINGS {
            let mut m = make(setting, 0.0);
            for n in 0..20_000 {
                let x = (std::f32::consts::TAU * 330.0 * n as f32 / SR).sin() * 5.0;
                let (l, r) = step(&mut m, x);
                // The reverb's dry gain ramps up over its first block.
                if n < 100 {
                    continue;
                }
                assert!(
                    (l - x).abs() < 1e-5 && (r - x).abs() < 1e-5,
                    "{setting:?} {n}: {x} → {l}, {r}"
                );
            }
        }
    }

    #[test]
    fn quiet_signals_pass_uncoloured_at_every_setting() {
        // 1 mV is below one 12-bit step of the hardware's full scale.
        for setting in SETTINGS {
            let mut m = make(setting, 0.0);
            let mut last = (0.0, 0.0);
            for _ in 0..20_000 {
                last = step(&mut m, 0.001);
            }
            assert!((last.0 - 0.001).abs() < 1e-6, "{setting:?} {last:?}");
            assert!((last.1 - 0.001).abs() < 1e-6, "{setting:?} {last:?}");
        }
    }

    #[test]
    fn reverb_adds_a_tail_at_every_setting() {
        for setting in SETTINGS {
            let mut m = make(setting, 4.0);
            for n in 0..4_800 {
                let x = (std::f32::consts::TAU * 330.0 * n as f32 / SR).sin() * 2.0;
                step(&mut m, x);
            }
            let mut energy = 0.0;
            for _ in 0..24_000 {
                let (l, r) = step(&mut m, 0.0);
                assert!(l.is_finite() && r.is_finite(), "{setting:?}");
                energy += l * l + r * r;
            }
            assert!(energy > 1.0, "{setting:?}: tail energy {energy}");
        }
    }

    #[test]
    fn moving_size_carries_the_tail() {
        let mut m = make(DIGITAL, 4.0);
        for n in 0..4_800 {
            let x = (std::f32::consts::TAU * 330.0 * n as f32 / SR).sin() * 2.0;
            step(&mut m, x);
        }
        // Sweep the size up and down across the tail, with an audio-rate
        // wobble on top.
        let mut energy = 0.0;
        for n in 0..48_000 {
            let sweep = (std::f32::consts::TAU * n as f32 / 48_000.0).sin().abs() * 5.0;
            let wobble = (std::f32::consts::TAU * 200.0 * n as f32 / SR).sin();
            m.params.size = volts(sweep + wobble);
            let (l, r) = step(&mut m, 0.0);
            assert!(l.is_finite() && r.is_finite() && l.abs() < 5.0 && r.abs() < 5.0);
            energy += l * l + r * r;
        }
        assert!(energy > 1.0, "tail energy {energy}");
    }

    #[test]
    fn non_finite_controls_keep_the_output_finite() {
        let mut m = make(DIGITAL, 4.0);
        m.params.amount = volts(f32::NAN);
        m.params.size = volts(f32::INFINITY);
        m.params.tone = volts(f32::NAN);
        for n in 0..10_000 {
            let x = (std::f32::consts::TAU * 330.0 * n as f32 / SR).sin() * 2.0;
            let (l, r) = step(&mut m, x);
            assert!(l.is_finite() && r.is_finite(), "{n}");
        }
    }
}
