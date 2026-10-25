//! `$wowFlutter`: Beads' dirty-tape wow and flutter at the engine rate.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::poly::{PolyOutput, PolySignal};

use super::wow_flutter::{WowModulation, write_read};
use super::{FULL_SCALE_VOLTS, input_value};

/// Rate the hardware runs its wow and flutter at.
const HARDWARE_RATE: f32 = 24000.0;
/// Longest modulated delay, in samples at [`HARDWARE_RATE`].
const MAX_HARDWARE_DELAY: f32 = 124.0;
/// The hardware's low-pass coefficient at [`HARDWARE_RATE`].
const HARDWARE_LP: f32 = 0.95;
/// Control blocks per second.
const BLOCK_RATE: f32 = 4000.0;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct WowFlutterParams {
    /// audio input
    input: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct WowFlutterOutputs {
    #[output("output", "the input with wow and flutter", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// One channel's delay line and low-pass.
#[derive(Default)]
struct Line {
    /// Power-of-two length covering the longest delay at the engine rate.
    line: Box<[f32]>,
    write_ptr: usize,
    lp_state: f32,
}

#[derive(Default)]
struct WowFlutterState {
    modulation: WowModulation,
    rng: LcgRng,
    /// Engine samples per hardware sample.
    ratio: f32,
    /// Low-pass coefficient at the engine rate.
    lp: f32,
    block_size: usize,
    block_pos: usize,
}

/// Tape wow and flutter from Mutable Instruments Beads' dirty tape setting:
/// slow, drifting pitch wobble with a slight softening of the highs. Every
/// channel wobbles together, like one tape carrying them all.
///
/// ```js
/// const grains = $pulse('8hz')
/// const cloud = $granular($saw('a3'), grains, { mix: 4, quality: 'dirtyTape' })
/// $reverb($wowFlutter(cloud), 3, { size: 5, tone: 2 }).out()
/// ```
#[module(name = "$wowFlutter", has_init, args(input))]
pub struct WowFlutter {
    outputs: WowFlutterOutputs,
    state: WowFlutterState,
    params: WowFlutterParams,
    channel_state: Box<[Line]>,
}

impl WowFlutter {
    /// Allocate each channel's delay line for `sample_rate`. Main thread.
    fn init(&mut self, sample_rate: f32) {
        let seed = seed_base(self);
        super::primitives::warm_tables();
        let state = &mut self.state;
        state.ratio = sample_rate / HARDWARE_RATE;
        state.lp = 1.0 - (1.0 - HARDWARE_LP).powf(HARDWARE_RATE / sample_rate);
        state.modulation.init(sample_rate);
        state.rng.seed(seed, 0);
        state.block_size = ((sample_rate / BLOCK_RATE) as usize).max(1);
        state.block_pos = 0;
        let len = ((MAX_HARDWARE_DELAY * state.ratio) as usize + 2).next_power_of_two();
        for line in self.channel_state.iter_mut() {
            line.line = vec![0.0; len].into_boxed_slice();
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let state = &mut self.state;
        if state.block_pos == 0 {
            state
                .modulation
                .start_block(state.block_size, &mut state.rng);
        }
        state.block_pos = (state.block_pos + 1) % state.block_size;
        let delay = state.modulation.next() * state.ratio;
        for (ch, line) in self.channel_state.iter_mut().enumerate() {
            let x = input_value(&self.params.input, ch);
            let y = write_read(&mut line.line, &mut line.write_ptr, x, delay);
            line.lp_state += state.lp * (y - line.lp_state);
            self.outputs
                .sample
                .set(ch, line.lp_state * FULL_SCALE_VOLTS);
        }
    }
}

message_handlers!(impl WowFlutter {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48000.0;

    fn make(channels: usize, sample_rate: f32) -> WowFlutter {
        let mut outputs = WowFlutterOutputs::default();
        outputs.set_all_channels(channels);
        let mut m = WowFlutter {
            outputs,
            state: Default::default(),
            params: WowFlutterParams {
                input: PolySignal::poly(&vec![Signal::Volts(0.0); channels]),
            },
            channel_state: (0..channels).map(|_| Line::default()).collect(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        m.init(sample_rate);
        m
    }

    /// Feed each channel's signal from `input` for `n` samples and return
    /// every channel's output.
    fn run(m: &mut WowFlutter, n: usize, input: impl Fn(usize, usize) -> f32) -> Vec<Vec<f32>> {
        let channels = m.channel_state.len();
        let mut out = vec![Vec::new(); channels];
        for i in 0..n {
            let signals: Vec<Signal> = (0..channels).map(|c| Signal::Volts(input(i, c))).collect();
            m.params.input = PolySignal::poly(&signals);
            m.update(SR);
            for (c, o) in out.iter_mut().enumerate() {
                o.push(m.outputs.sample.get(c));
            }
        }
        out
    }

    #[test]
    fn passes_a_steady_level_unchanged() {
        let mut m = make(1, SR);
        let out = run(&mut m, 48_000, |_, _| 2.0);
        assert!((out[0][47_999] - 2.0).abs() < 1e-4, "{}", out[0][47_999]);
    }

    #[test]
    fn channels_share_one_modulation() {
        let mut m = make(3, SR);
        let out = run(&mut m, 48_000, |i, _| (i as f32 * 0.05).sin());
        assert_eq!(out[0], out[1]);
        assert_eq!(out[0], out[2]);
    }

    /// Positive-going zero crossings of `xs`, interpolated.
    fn crossings(xs: &[f32]) -> Vec<f64> {
        (1..xs.len())
            .filter(|&i| xs[i - 1] < 0.0 && xs[i] >= 0.0)
            .map(|i| (i - 1) as f64 + xs[i - 1] as f64 / (xs[i - 1] - xs[i]) as f64)
            .collect()
    }

    #[test]
    fn wobbles_the_pitch() {
        let mut m = make(1, SR);
        let out = run(&mut m, 4 * 48_000, |i, _| {
            (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()
        });
        let c = crossings(&out[0][48_000..]);
        let periods: Vec<f64> = c.windows(2).map(|w| w[1] - w[0]).collect();
        let (min, max) = periods
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), &p| (lo.min(p), hi.max(p)));
        // A steady 1 kHz tone is 48 samples per cycle; the wobble stretches
        // and squeezes it.
        assert!(max - min > 0.1, "periods {min}..{max}");
        assert!((min - 48.0).abs() < 3.0 && (max - 48.0).abs() < 3.0);
    }

    #[test]
    fn delay_line_covers_the_longest_delay_at_high_rates() {
        let m = make(1, 192_000.0);
        assert!(m.channel_state[0].line.len() as f32 > MAX_HARDWARE_DELAY * 8.0 + 1.0);
    }
}
