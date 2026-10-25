use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{CatmullRom, SchmittState, SchmittTrigger};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// Rate of new random points at `speed = 5`, in Hz, unless the engine rate is
/// higher. At `speed = 5` every sample draws a fresh point, so the output is
/// white noise and held values are uniform.
const MAX_RATE_HZ: f32 = 48_000.0;

/// Octaves spanned by the speed curve: `speed` 0–5 sweeps the point rate
/// exponentially across this many octaves below the top rate, reaching
/// exactly 0 Hz at `speed = 0`.
const SPEED_OCTAVES: f32 = 24.0;

/// Width of the default ±5 V range. `amount = 5` spans this full width.
const NOMINAL_WIDTH: f32 = 10.0;

/// In sparse mode, the depth drawn for a point is `min(1, SPARSE_SCALE · a · u)`
/// with `a = amount / 5` and `u` uniform in [0, 1), so past `a = 1/3` a share
/// `1 − 1/(3a)` of the draws reach full depth.
const SPARSE_SCALE: f32 = 3.0;

/// Points per sample for a `speed` in 0–5. Reaches exactly 1 at `speed = 5`
/// whatever the sample rate.
#[inline]
fn speed_to_increment(speed: f32, sample_rate: f32) -> f32 {
    let s = speed.clamp(0.0, 5.0) / 5.0;
    if s >= 1.0 {
        return 1.0;
    }
    let curve = ((SPEED_OCTAVES * s).exp2() - 1.0) / (SPEED_OCTAVES.exp2() - 1.0);
    (curve * MAX_RATE_HZ.max(sample_rate) / sample_rate).min(1.0)
}

/// How the random value moves the input.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
enum ErraticMode {
    /// Add a random offset.
    #[default]
    Shift,
    /// Move toward a random value in the range.
    Blend,
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ErraticParams {
    /// signal to shift
    input: PolySignal,
    /// depth (0–5); at 5, shift moves up to half the range either way and
    /// blend jumps all the way to the random value
    #[signal(range = (0.0, 5.0))]
    amount: PolySignal,
    /// rising edge samples and holds the current random shift
    #[signal(type = trig, range = (0.0, 5.0))]
    #[block_input(optional)]
    #[deserr(default)]
    trigger: Option<PolySignal>,
    /// how fast the shift wanders: 0 = frozen, 5 = white noise
    #[signal(default = 2.0, range = (0.0, 5.0))]
    #[deserr(default)]
    speed: Option<PolySignal>,
    /// skews the shift upward (positive) or downward (negative); ±5 makes it one-sided
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    bias: Option<PolySignal>,
    /// minimum of the input's range
    #[signal(default = -5.0)]
    #[deserr(default)]
    min: Option<PolySignal>,
    /// maximum of the input's range
    #[signal(default = 5.0)]
    #[deserr(default)]
    max: Option<PolySignal>,
    /// clamp the output to [min, max]
    #[serde(default)]
    #[deserr(default)]
    clamp: bool,
    /// shift adds a random offset; blend moves toward a random value in the range
    #[serde(default)]
    #[deserr(default)]
    mode: ErraticMode,
    /// mostly stay close to the input, with occasional big jumps
    #[serde(default)]
    #[deserr(default)]
    sparse: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ErraticOutputs {
    #[output("output", "randomly shifted signal", default)]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ErraticChannelState {
    rng: LcgRng,
    seeded: bool,
    /// Position between `points[1]` and `points[2]`, in [0, 1). f64, so the
    /// tiny increments of low speeds still advance it near 1.
    phase: f64,
    /// The last four random points in [-1, 1], oldest first.
    points: [f32; 4],
    /// The curve through `points`, from the second toward the third.
    curve: CatmullRom,
    /// The last four sparse-depth draws in [0, 1], drawn alongside `points`.
    sparse_points: [f32; 4],
    sparse_curve: CatmullRom,
    trigger: SchmittTrigger,
    /// The held point and sparse draw.
    held: (f32, f32),
}

/// One channel's inputs for one sample.
struct ChannelInputs {
    input: f32,
    amount: f32,
    /// `None` when no trigger is connected.
    trigger: Option<f32>,
    speed: f32,
    bias: f32,
    min: f32,
    max: f32,
}

impl ErraticChannelState {
    /// Seed this channel's RNG from a [`seed_base`] and its channel index, and
    /// draw its first four points.
    fn seed(&mut self, base: u64, channel: usize) {
        self.rng.seed(base, channel);
        for i in 0..4 {
            self.points[i] = self.rng.next_bipolar();
            self.sparse_points[i] = self.rng.next_unit();
        }
        self.curve = CatmullRom::new(self.points);
        self.sparse_curve = CatmullRom::new(self.sparse_points);
        self.seeded = true;
    }

    /// Advance one sample and return the randomised input.
    #[inline(always)]
    fn step(&mut self, f: ChannelInputs, p: &ErraticParams, sample_rate: f32) -> f32 {
        let inc = speed_to_increment(f.speed, sample_rate);
        self.phase += f64::from(inc);
        if self.phase >= 1.0 {
            // A point per sample stays on the fresh point itself, so the
            // output is the uniform draws rather than a curve between them.
            self.phase = if inc >= 1.0 { 0.0 } else { self.phase - 1.0 };
            let [_, a, b, c] = self.points;
            self.points = [a, b, c, self.rng.next_bipolar()];
            self.curve = CatmullRom::new(self.points);
            let [_, a, b, c] = self.sparse_points;
            self.sparse_points = [a, b, c, self.rng.next_unit()];
            self.sparse_curve = CatmullRom::new(self.sparse_points);
        }
        let t = self.phase as f32;
        let wander = (
            self.curve.at(t).clamp(-1.0, 1.0),
            self.sparse_curve.at(t).clamp(0.0, 1.0),
        );

        let (r, u) = match f.trigger {
            Some(trig) => {
                // The first sample latches too, so the hold starts at a random
                // value rather than at the input.
                let uninit = self.trigger.state == SchmittState::Uninitialized;
                if self.trigger.process(trig) || uninit {
                    self.held = wander;
                }
                self.held
            }
            None => wander,
        };

        let (lo_bound, hi_bound) = if f.max < f.min {
            (f.max, f.min)
        } else {
            (f.min, f.max)
        };
        let depth = f.amount.max(0.0) / 5.0;
        let depth = if p.sparse {
            (SPARSE_SCALE * depth * u).min(1.0)
        } else {
            depth
        };
        let bias = f.bias.clamp(-5.0, 5.0) / 5.0;

        let out = match p.mode {
            ErraticMode::Shift => {
                let half = depth * 5.0 * (hi_bound - lo_bound) / NOMINAL_WIDTH;
                let lo = -half * (1.0 - bias.max(0.0));
                let hi = half * (1.0 + bias.min(0.0));
                f.input + lo + (hi - lo) * (r + 1.0) * 0.5
            }
            ErraticMode::Blend => {
                // Bias narrows the target span toward the input from one side.
                let anchor = f.input.clamp(lo_bound, hi_bound);
                let lo = lo_bound + (anchor - lo_bound) * bias.max(0.0);
                let hi = hi_bound - (hi_bound - anchor) * (-bias).max(0.0);
                let target = lo + (hi - lo) * (r + 1.0) * 0.5;
                f.input + (target - f.input) * depth.min(1.0)
            }
        };
        if p.clamp {
            out.clamp(lo_bound, hi_bound)
        } else {
            out
        }
    }
}

/// Applies a wandering random variation to a signal.
///
/// **shift** mode adds an offset of up to ±**amount**, scaled to the width of
/// [**min**, **max**]: 5 moves up to half the range either way. **blend** mode
/// moves toward a random value in [**min**, **max**] by **amount** / 5, so it
/// never leaves the range. **bias** makes the variation lean upward (+) or
/// downward (−); ±5 makes it one-sided. **sparse** keeps most values close to
/// the input, with occasional big jumps. **speed** runs from frozen at 0 to a
/// fresh random value every sample at 5. A **trigger** holds the variation
/// between rising edges.
///
/// Backs the DSL's `.erratic()` signal method, which fills **min**/**max**
/// from a ranged signal's range.
#[module(name = "_erratic", patch_update, args(input, amount), block)]
pub struct Erratic {
    outputs: ErraticOutputs,
    params: ErraticParams,
    channel_state: Box<[ErraticChannelState]>,
}

impl Erratic {
    fn update(&mut self, sample_rate: f32) {
        let p = &self.params;
        for ch in 0..self.channel_count() {
            let frame = ChannelInputs {
                input: p.input.get_value(ch),
                amount: p.amount.get_value(ch),
                trigger: p.trigger.as_ref().map(|trigger| trigger.get_value(ch)),
                speed: p.speed.value_or(ch, ErraticParams::SPEED_DEFAULT),
                bias: p.bias.value_or(ch, ErraticParams::BIAS_DEFAULT),
                min: p.min.value_or(ch, ErraticParams::MIN_DEFAULT),
                max: p.max.value_or(ch, ErraticParams::MAX_DEFAULT),
            };
            let value = self.channel_state[ch].step(frame, p, sample_rate);
            self.outputs.sample.set(ch, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut ErraticBlockOutputs,
        range: Range<usize>,
        sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), channels, &mut scratch)?;
        for i in range {
            let [input, amount, speed, bias, min, max] = [
                &inputs.input,
                &inputs.amount,
                &inputs.speed,
                &inputs.bias,
                &inputs.min,
                &inputs.max,
            ]
            .map(|input| input.row(i));
            let trigger = inputs.trigger.as_ref().map(|input| input.row(i));
            for (ch, value) in outputs.sample.slot_mut(i).iter_mut().enumerate() {
                let frame = ChannelInputs {
                    input: input[ch],
                    amount: amount[ch],
                    trigger: trigger.map(|row| row[ch]),
                    speed: speed[ch],
                    bias: bias[ch],
                    min: min[ch],
                    max: max[ch],
                };
                *value = self.channel_state[ch].step(frame, &self.params, sample_rate);
            }
        }
        Some(())
    }
}

impl crate::types::PatchUpdateHandler for Erratic {
    /// Seeds each unseeded channel off the module's stable heap address. The
    /// per-channel `seeded` flag rides `transfer_state_from`, so a running
    /// channel keeps its sequence across patch updates while newly added
    /// channels still get distinct streams.
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            if !state.seeded {
                state.seed(base, ch);
            }
        }
    }
}

message_handlers!(impl Erratic {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, PatchUpdateHandler, Signal};

    const SR: f32 = 48_000.0;

    fn volts(v: f32) -> PolySignal {
        PolySignal::mono(Signal::Volts(v))
    }

    fn params(input: f32, amount: f32, speed: f32, bias: f32) -> ErraticParams {
        ErraticParams {
            input: volts(input),
            amount: volts(amount),
            trigger: None,
            speed: Some(volts(speed)),
            bias: Some(volts(bias)),
            min: None,
            max: None,
            clamp: false,
            mode: ErraticMode::Shift,
            sparse: false,
        }
    }

    fn make(params: ErraticParams) -> Erratic {
        let channels = __erratic_derive_channel_count(&params);
        let mut outputs = ErraticOutputs::default();
        outputs.set_all_channels(channels);
        let mut m = Erratic {
            params,
            outputs,
            channel_state: vec![ErraticChannelState::default(); channels].into_boxed_slice(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        m.on_patch_update();
        m
    }

    fn run(m: &mut Erratic, n: usize) -> Vec<f32> {
        run_at(m, n, SR)
    }

    fn run_at(m: &mut Erratic, n: usize, sample_rate: f32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                m.update(sample_rate);
                m.outputs.sample.get(0)
            })
            .collect()
    }

    /// Share of `xs` in each of `bins` equal bins across [lo, hi].
    fn histogram(xs: &[f32], lo: f32, hi: f32, bins: usize) -> Vec<f32> {
        let mut counts = vec![0usize; bins];
        for &x in xs {
            let i = ((x - lo) / (hi - lo) * bins as f32) as usize;
            counts[i.min(bins - 1)] += 1;
        }
        counts
            .into_iter()
            .map(|c| c as f32 / xs.len() as f32)
            .collect()
    }

    fn bounds(xs: &[f32]) -> (f32, f32) {
        xs.iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)))
    }

    #[test]
    fn zero_amount_passes_input_through() {
        let mut m = make(params(1.25, 0.0, 5.0, 0.0));
        assert!(run(&mut m, 256).iter().all(|&x| x == 1.25));
    }

    #[test]
    fn zero_speed_is_immobile() {
        let mut m = make(params(0.0, 5.0, 0.0, 0.0));
        let xs = run(&mut m, 4096);
        assert!(xs.iter().all(|&x| x == xs[0]));
    }

    #[test]
    fn full_speed_draws_a_new_point_every_sample() {
        let mut m = make(params(0.0, 5.0, 5.0, 0.0));
        let xs = run(&mut m, 1024);
        assert!(xs.windows(2).all(|w| w[0] != w[1]));
        let (lo, hi) = bounds(&xs);
        assert!(
            lo < -4.0 && hi > 4.0,
            "white noise should fill ±5: {lo}..{hi}"
        );
    }

    #[test]
    fn shift_scales_with_range_width() {
        let mut p = params(-2.0, 5.0, 5.0, 0.0);
        p.min = Some(volts(-3.0));
        p.max = Some(volts(-1.0));
        let mut m = make(p);
        let (lo, hi) = bounds(&run(&mut m, 4096));
        assert!(lo >= -3.0 && hi <= -1.0, "{lo}..{hi}");
        assert!(lo < -2.9 && hi > -1.1, "{lo}..{hi}");
    }

    #[test]
    fn bias_makes_shift_one_sided() {
        let mut up = make(params(0.0, 2.5, 5.0, 5.0));
        let (lo, hi) = bounds(&run(&mut up, 4096));
        assert!(lo >= 0.0 && hi <= 2.5 && hi > 2.4, "{lo}..{hi}");

        let mut down = make(params(0.0, 2.5, 5.0, -2.5));
        let (lo, hi) = bounds(&run(&mut down, 4096));
        assert!(
            (-2.5..-2.4).contains(&lo) && hi <= 1.25 && hi > 1.15,
            "{lo}..{hi}"
        );
    }

    #[test]
    fn clamp_holds_output_in_range() {
        let mut p = params(4.5, 5.0, 5.0, 5.0);
        p.min = Some(volts(0.0));
        p.max = Some(volts(5.0));
        p.clamp = true;
        let mut m = make(p);
        let (lo, hi) = bounds(&run(&mut m, 1024));
        assert!(lo >= 4.5 && hi <= 5.0, "{lo}..{hi}");
    }

    #[test]
    fn trigger_holds_shift_between_edges() {
        let mut p = params(0.0, 5.0, 5.0, 0.0);
        p.trigger = Some(volts(0.0));
        let mut m = make(p);
        let xs = run(&mut m, 512);
        assert!(xs.iter().all(|&x| x == xs[0]));

        m.params.trigger = Some(volts(5.0));
        let after = run(&mut m, 512);
        assert_ne!(after[0], xs[0]);
        assert!(after.iter().all(|&x| x == after[0]));
    }

    /// `process_block` over ranges of varying length matches `update` sample
    /// for sample, from the same seeded state, with per-channel constants and a
    /// trigger that rises mid-run.
    #[test]
    fn block_path_matches_update() {
        let poly = |vs: &[f32]| {
            PolySignal::poly(&vs.iter().map(|&v| Signal::Volts(v)).collect::<Vec<_>>())
        };
        let mut p = params(0.0, 0.0, 0.0, 0.0);
        p.input = poly(&[0.1, -0.2, 0.3, 4.9]);
        p.amount = poly(&[1.0, 2.5, 5.0, 3.0]);
        p.speed = Some(poly(&[2.0, 3.5, 4.5, 5.0]));
        p.bias = Some(poly(&[0.0, 2.0, -3.0, 5.0]));
        p.min = Some(poly(&[-5.0, 0.0, -3.0, 0.0]));
        p.max = Some(poly(&[5.0, 1.0, -1.0, 5.0]));
        p.clamp = true;
        let cases = [false, true].into_iter().flat_map(|trigger| {
            [ErraticMode::Shift, ErraticMode::Blend]
                .into_iter()
                .flat_map(move |mode| [false, true].map(|sparse| (trigger, mode, sparse)))
        });
        for (trigger, mode, sparse) in cases {
            let mut p = p.clone();
            p.trigger = trigger.then(|| volts(0.0));
            p.mode = mode;
            p.sparse = sparse;
            let mut sample = make(p.clone());
            let mut block = make(p);
            block.channel_state.copy_from_slice(&sample.channel_state);

            let channels = sample.channel_count();
            let mut outputs = ErraticBlockOutputs::new(64, channels);
            let mut expected = Vec::new();
            let mut actual = Vec::new();
            for (n, len) in [1, 9, 1, 30, 23].into_iter().cycle().take(20).enumerate() {
                let start = n % 3 * 2;
                if trigger {
                    let level = volts(if n % 4 < 2 { 0.0 } else { 5.0 });
                    sample.params.trigger = Some(level.clone());
                    block.params.trigger = Some(level);
                }
                for _ in 0..len {
                    sample.update(SR);
                    expected.extend((0..channels).map(|ch| sample.outputs.sample.get(ch)));
                }
                block
                    .process_block(&mut outputs, start..start + len, SR)
                    .unwrap();
                actual
                    .extend((start..start + len).flat_map(|i| outputs.sample.slot_mut(i).to_vec()));
            }
            assert!(expected.windows(2).any(|w| w[0] != w[1]));
            assert_eq!(
                actual, expected,
                "trigger connected: {trigger}, {mode:?}, sparse: {sparse}"
            );
        }
    }

    /// A speed whose per-sample increment is below an f32 step near 1 still
    /// moves the shift at its full rate.
    #[test]
    fn low_speed_keeps_moving() {
        let speed = 0.1;
        let mut m = make(params(0.0, 5.0, speed, 0.0));
        m.channel_state[0].phase = 0.75;
        let samples = 48_000;
        run(&mut m, samples);
        let expected = 0.75 + f64::from(speed_to_increment(speed, SR)) * samples as f64;
        assert!((m.channel_state[0].phase - expected).abs() < 1e-9);
    }

    #[test]
    fn speed_curve_endpoints() {
        for sample_rate in [44_100.0, SR, 96_000.0] {
            assert_eq!(speed_to_increment(0.0, sample_rate), 0.0);
            assert_eq!(speed_to_increment(5.0, sample_rate), 1.0);
        }
        let near_top = speed_to_increment(4.9, SR) * SR;
        assert!(near_top < MAX_RATE_HZ && near_top > MAX_RATE_HZ / 2.0);
    }

    /// At an engine rate above [`MAX_RATE_HZ`], speed 5 still draws a fresh
    /// point every sample, even when the phase sat mid-curve beforehand.
    #[test]
    fn full_speed_draws_fresh_points_at_high_sample_rates() {
        let mut m = make(params(0.0, 5.0, 5.0, 0.0));
        m.channel_state[0].phase = 0.4;
        run_at(&mut m, 4, 96_000.0);
        let state = m.channel_state[0];
        assert_eq!(state.phase, 0.0);
        assert!((m.outputs.sample.get(0) - 5.0 * state.points[1]).abs() < 1e-5);
    }

    #[test]
    fn blend_pulls_toward_a_uniform_point_in_range() {
        let mut p = params(4.0, 5.0, 5.0, 0.0);
        p.mode = ErraticMode::Blend;
        p.min = Some(volts(0.0));
        p.max = Some(volts(5.0));
        let xs = run(&mut make(p.clone()), 100_000);
        let (lo, hi) = bounds(&xs);
        assert!(lo >= 0.0 && hi <= 5.0, "{lo}..{hi}");
        for share in histogram(&xs, 0.0, 5.0, 10) {
            assert!((share - 0.1).abs() < 0.01, "not uniform: {share}");
        }

        p.amount = volts(2.5);
        let (lo, hi) = bounds(&run(&mut make(p), 100_000));
        assert!(
            (lo - 2.0).abs() < 0.01 && (hi - 4.5).abs() < 0.01,
            "{lo}..{hi}"
        );
    }

    #[test]
    fn blend_bias_targets_one_side_of_the_input() {
        let mut p = params(1.0, 5.0, 5.0, 5.0);
        p.mode = ErraticMode::Blend;
        let (lo, hi) = bounds(&run(&mut make(p.clone()), 10_000));
        assert!(lo >= 1.0 && lo < 1.1 && hi > 4.9, "{lo}..{hi}");

        p.bias = Some(volts(-5.0));
        let (lo, hi) = bounds(&run(&mut make(p), 10_000));
        assert!(lo < -4.9 && hi <= 1.0 && hi > 0.9, "{lo}..{hi}");
    }

    /// Sparse depth is `min(1, 3a·u)`: at full amount two thirds of the draws
    /// land at full depth and the rest spread evenly below it.
    #[test]
    fn sparse_draws_full_depth_for_a_share_of_points() {
        let mut p = params(0.0, 5.0, 5.0, 5.0);
        p.mode = ErraticMode::Blend;
        p.min = Some(volts(0.0));
        p.max = Some(volts(1.0));
        p.sparse = true;
        let mut m = make(p);
        let n = 100_000;
        let mut full = 0;
        for _ in 0..n {
            m.update(SR);
            let state = m.channel_state[0];
            let target = (state.points[1] + 1.0) * 0.5;
            let out = m.outputs.sample.get(0);
            if (out - target).abs() < 1e-6 {
                full += 1;
            }
        }
        let share = full as f32 / n as f32;
        assert!((share - 2.0 / 3.0).abs() < 0.01, "{share}");
    }

    /// Sparse amount 1 peaks at ±3 V, like dense amount 3, but concentrates
    /// near the input: within 0.5 V for a share `c(1 − ln c)` with `c = 1/6`,
    /// against 1/6 for the dense shift.
    #[test]
    fn sparse_shift_mostly_stays_near_the_input() {
        let mut p = params(0.0, 1.0, 5.0, 0.0);
        p.sparse = true;
        let sparse = run(&mut make(p), 100_000);
        let dense = run(&mut make(params(0.0, 3.0, 5.0, 0.0)), 100_000);
        let near =
            |xs: &[f32]| xs.iter().filter(|x| x.abs() < 0.5).count() as f32 / xs.len() as f32;
        let c: f32 = 1.0 / 6.0;
        assert!((near(&dense) - c).abs() < 0.01, "{}", near(&dense));
        assert!(
            (near(&sparse) - c * (1.0 - c.ln())).abs() < 0.01,
            "{}",
            near(&sparse)
        );
        let (lo, hi) = bounds(&sparse);
        assert!(
            lo < -2.9 && hi > 2.9 && lo >= -3.0 && hi <= 3.0,
            "{lo}..{hi}"
        );
    }
}
