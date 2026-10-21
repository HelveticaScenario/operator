use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{CatmullRom, SchmittState, SchmittTrigger, sanitize};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// Rate of new random points at `speed = 5`, in Hz. At or above the engine
/// rate every sample draws a fresh point, so the shift is white noise.
const MAX_RATE_HZ: f32 = 48_000.0;

/// Octaves spanned by the speed curve: `speed` 0–5 sweeps the point rate
/// exponentially across this many octaves below [`MAX_RATE_HZ`], reaching
/// exactly 0 Hz at `speed = 0`.
const SPEED_OCTAVES: f32 = 24.0;

/// Width of the default ±5 V range. `amount = 5` spans this full width.
const NOMINAL_WIDTH: f32 = 10.0;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ErraticParams {
    /// signal to shift
    input: PolySignal,
    /// shift depth (0–5); 5 shifts by up to half the range in each direction
    #[signal(range = (0.0, 5.0))]
    amount: PolySignal,
    /// rising edge samples and holds the current random shift
    #[signal(type = trig, range = (0.0, 5.0))]
    #[block_input(optional)]
    #[deserr(default)]
    trigger: Option<PolySignal>,
    /// how fast the shift wanders (0 = frozen, 5 = white noise)
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
    trigger: SchmittTrigger,
    held: f32,
}

/// One channel's inputs for one sample.
struct Frame {
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
    /// Advance one sample and return the shifted input.
    #[inline(always)]
    fn step(&mut self, f: Frame, clamp: bool, sample_rate: f32) -> f32 {
        let inc = (speed_to_hz(f.speed) / sample_rate).min(1.0);
        self.phase += f64::from(inc);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.points = [
                self.points[1],
                self.points[2],
                self.points[3],
                self.rng.next_bipolar(),
            ];
            self.curve = CatmullRom::new(self.points);
        }
        let wander = self.curve.at(self.phase as f32).clamp(-1.0, 1.0);

        let r = match f.trigger {
            Some(trig) => {
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
        let half = f.amount.max(0.0) * (hi_bound - lo_bound) / NOMINAL_WIDTH;
        let bias = f.bias.clamp(-5.0, 5.0) / 5.0;
        let lo = -half * (1.0 - bias.max(0.0));
        let hi = half * (1.0 + bias.min(0.0));
        let shift = lo + (hi - lo) * (r + 1.0) * 0.5;

        let out = f.input + shift;
        if clamp {
            out.clamp(lo_bound, hi_bound)
        } else {
            out
        }
    }
}

/// Applies a wandering random shift to a signal.
///
/// The shift spans ±**amount** scaled to the width of [**min**, **max**]: an
/// amount of 5 moves the signal by up to half the range either way. **bias**
/// shrinks one side of that span — at +5 the shift is only upward, at -5 only
/// downward. **speed** sets how quickly the shift changes, from frozen at 0
/// through smooth drift to white noise at 5. A **trigger** samples and holds
/// the shift on each rising edge.
///
/// Backs the DSL's `.erratic()` signal method, which fills **min**/**max**
/// from a ranged signal's range.
#[module(name = "_erratic", patch_update, args(input, amount), block)]
pub struct Erratic {
    outputs: ErraticOutputs,
    params: ErraticParams,
    channel_state: Box<[ErraticChannelState]>,
}

/// Shift-point rate for a `speed` in 0–5, in Hz.
#[inline]
fn speed_to_hz(speed: f32) -> f32 {
    let s = speed.clamp(0.0, 5.0) / 5.0;
    MAX_RATE_HZ * ((SPEED_OCTAVES * s).exp2() - 1.0) / (SPEED_OCTAVES.exp2() - 1.0)
}

impl Erratic {
    fn update(&mut self, sample_rate: f32) {
        let p = &self.params;
        for ch in 0..self.channel_count() {
            let frame = Frame {
                input: p.input.get_value(ch),
                amount: p.amount.get_value(ch),
                trigger: p.trigger.as_ref().map(|trigger| trigger.get_value(ch)),
                speed: p.speed.value_or(ch, ErraticParams::SPEED_DEFAULT),
                bias: p.bias.value_or(ch, ErraticParams::BIAS_DEFAULT),
                min: p.min.value_or(ch, ErraticParams::MIN_DEFAULT),
                max: p.max.value_or(ch, ErraticParams::MAX_DEFAULT),
            };
            let value = self.channel_state[ch].step(frame, p.clamp, sample_rate);
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
                let frame = Frame {
                    input: input[ch],
                    amount: amount[ch],
                    trigger: trigger.map(|row| row[ch]),
                    speed: speed[ch],
                    bias: bias[ch],
                    min: min[ch],
                    max: max[ch],
                };
                *value =
                    sanitize(self.channel_state[ch].step(frame, self.params.clamp, sample_rate));
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
                state.rng.seed(base, ch);
                state.points = std::array::from_fn(|_| state.rng.next_bipolar());
                state.curve = CatmullRom::new(state.points);
                state.seeded = true;
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
        (0..n)
            .map(|_| {
                m.update(SR);
                m.outputs.sample.get(0)
            })
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
        for trigger in [false, true] {
            let mut p = p.clone();
            p.trigger = trigger.then(|| volts(0.0));
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
            assert_eq!(actual, expected, "trigger connected: {trigger}");
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
        let expected = 0.75 + f64::from(speed_to_hz(speed) / SR) * samples as f64;
        assert!((m.channel_state[0].phase - expected).abs() < 1e-9);
    }

    #[test]
    fn speed_curve_endpoints() {
        assert_eq!(speed_to_hz(0.0), 0.0);
        assert!((speed_to_hz(5.0) - MAX_RATE_HZ).abs() < 1.0);
    }
}
