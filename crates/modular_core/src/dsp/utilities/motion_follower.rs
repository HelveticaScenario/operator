use crate::{
    dsp::{shape::shapers::tanh_driven, utils::sanitize},
    poly::{PolyOutput, PolySignal, PolySignalExt},
};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct MotionFollowerParams {
    /// controller signal whose movement drives the output
    input: PolySignal,
    /// seconds for the level to rise toward the current speed (default 0.02)
    #[signal(default = 0.02, range = (0.0, 5.0))]
    #[deserr(default)]
    attack: Option<PolySignal>,
    /// seconds for the level to fall once movement slows or stops (default 0.3)
    #[signal(default = 0.3, range = (0.0, 10.0))]
    #[deserr(default)]
    release: Option<PolySignal>,
    /// seconds of averaging applied to the speed before attack/release; set it
    /// longer than the gap between controller updates to hide stepping (default 0.02)
    #[signal(default = 0.02, range = (0.0, 1.0))]
    #[deserr(default)]
    smooth: Option<PolySignal>,
    /// volts of output per volt-per-second of movement (default 0.25)
    #[signal(default = 0.25, range = (0.0, 10.0))]
    #[deserr(default)]
    gain: Option<PolySignal>,
    /// soft-clip ceiling in volts; the output never exceeds it (default 5)
    #[signal(default = 5.0, range = (0.0, 20.0))]
    #[deserr(default)]
    ceiling: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MotionFollowerOutputs {
    #[output("output", "movement level in either direction (0 V at rest)", default, range = (0.0, 5.0))]
    sample: PolyOutput,
    #[output("pos", "movement level from rising input only", range = (0.0, 5.0))]
    pos: PolyOutput,
    #[output("neg", "movement level from falling input only", range = (0.0, 5.0))]
    neg: PolyOutput,
}

/// One-pole coefficient cached against the time it was computed from, so a
/// constant time costs no `exp` per sample.
#[derive(Clone, Copy)]
struct CachedCoeff {
    secs: f32,
    coeff: f32,
}

impl Default for CachedCoeff {
    fn default() -> Self {
        // NaN never compares equal, so the first `get` always computes.
        Self {
            secs: f32::NAN,
            coeff: 1.0,
        }
    }
}

impl CachedCoeff {
    /// Coefficient for a one-pole with time constant `secs`; `secs <= 0` is instant.
    #[inline]
    fn get(&mut self, secs: f32, sample_rate: f32) -> f32 {
        if secs != self.secs {
            self.secs = secs;
            self.coeff = if secs > 0.0 {
                1.0 - (-1.0 / (secs * sample_rate)).exp()
            } else {
                1.0
            };
        }
        self.coeff
    }
}

/// Averages a raw speed, then follows it with attack/release.
#[derive(Default, Clone, Copy)]
struct Follower {
    speed: f32,
    level: f32,
}

impl Follower {
    #[inline]
    fn process(&mut self, raw_speed: f32, smooth_c: f32, attack_c: f32, release_c: f32) -> f32 {
        self.speed = sanitize(self.speed + (raw_speed - self.speed) * smooth_c);
        let follow_c = if self.speed > self.level {
            attack_c
        } else {
            release_c
        };
        self.level = sanitize(self.level + (self.speed - self.level) * follow_c);
        self.level
    }
}

#[derive(Default, Clone, Copy)]
struct MotionFollowerChannelState {
    prev: f32,
    initialized: bool,
    both: Follower,
    rising: Follower,
    falling: Follower,
    smooth: CachedCoeff,
    attack: CachedCoeff,
    release: CachedCoeff,
}

/// Turns controller movement into a level, like bowing a string: moving the
/// input in either direction swells the output, holding it still lets it fade.
///
/// The input's speed (volts per second, direction ignored) is averaged over
/// **smooth** seconds, then followed with separate **attack** and **release**
/// times. **gain** converts speed to volts and a soft clip keeps the output
/// below **ceiling**.
///
/// **pos** and **neg** are the same level driven only by rising or only by
/// falling input, so pushing and releasing a controller can steer different
/// destinations. Both are positive voltages.
///
/// Stepped controllers such as 7-bit MIDI CC jump in small increments, so
/// their raw speed is a train of spikes; a **smooth** time several times longer
/// than the gap between updates averages them into a steady level.
///
/// ```js
/// // a slowly swinging LFO bows a filter open on every sweep
/// $lpf($saw('c3'), $motionFollower($sine('0.5hz'), { release: 0.5 }).range('c3', 'c7')).out()
/// ```
#[module(name = "$motionFollower", args(input))]
pub struct MotionFollower {
    outputs: MotionFollowerOutputs,
    params: MotionFollowerParams,
    channel_state: Box<[MotionFollowerChannelState]>,
}

impl MotionFollower {
    pub fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];
            let input = sanitize(self.params.input.get_value(ch));
            if !state.initialized {
                state.prev = input;
                state.initialized = true;
            }

            let rate = (input - state.prev) * sample_rate;
            state.prev = input;

            let smooth_c = state
                .smooth
                .get(self.params.smooth.value_or(ch, 0.02), sample_rate);
            let attack_c = state
                .attack
                .get(self.params.attack.value_or(ch, 0.02), sample_rate);
            let release_c = state
                .release
                .get(self.params.release.value_or(ch, 0.3), sample_rate);

            let both = state
                .both
                .process(rate.abs(), smooth_c, attack_c, release_c);
            let rising = state
                .rising
                .process(rate.max(0.0), smooth_c, attack_c, release_c);
            let falling = state
                .falling
                .process((-rate).max(0.0), smooth_c, attack_c, release_c);

            let ceiling = self.params.ceiling.value_or(ch, 5.0).max(0.001);
            let gain = self.params.gain.value_or(ch, 0.25);
            let shape = |level: f32| sanitize(ceiling * tanh_driven(level * gain / ceiling));
            self.outputs.sample.set(ch, shape(both));
            self.outputs.pos.set(ch, shape(rising));
            self.outputs.neg.set(ch, shape(falling));
        }
    }
}

message_handlers!(impl MotionFollower {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Signal;

    const SR: f32 = 48000.0;

    fn volts(v: f32) -> PolySignal {
        PolySignal::mono(Signal::Volts(v))
    }

    /// A follower with unit gain and a ceiling high enough that the soft clip
    /// stays near-linear, so outputs read directly as speed in V/s.
    fn make(channels: usize) -> MotionFollower {
        let mut module = MotionFollower {
            outputs: MotionFollowerOutputs::default(),
            params: MotionFollowerParams {
                input: volts(0.0),
                attack: None,
                release: None,
                smooth: None,
                gain: Some(volts(1.0)),
                ceiling: Some(volts(1000.0)),
            },
            channel_state: vec![MotionFollowerChannelState::default(); channels].into_boxed_slice(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        module.outputs.sample.set_channels(channels);
        module.outputs.pos.set_channels(channels);
        module.outputs.neg.set_channels(channels);
        module
    }

    fn step(module: &mut MotionFollower, input: f32) -> f32 {
        module.params.input = volts(input);
        module.update(SR);
        module.outputs.sample.get(0)
    }

    /// Drives a linear ramp at `slope` V/s starting from `start` for `secs`;
    /// returns the final input and output.
    fn ramp(module: &mut MotionFollower, start: f32, slope: f32, secs: f32) -> (f32, f32) {
        let n = (secs * SR) as usize;
        let mut out = 0.0;
        let mut x = start;
        for i in 0..n {
            x = start + slope * i as f32 / SR;
            out = step(module, x);
        }
        (x, out)
    }

    fn hold(module: &mut MotionFollower, value: f32, secs: f32) -> f32 {
        let mut out = 0.0;
        for _ in 0..(secs * SR) as usize {
            out = step(module, value);
        }
        out
    }

    #[test]
    fn still_input_outputs_zero() {
        let mut m = make(1);
        assert_eq!(hold(&mut m, 3.0, 0.1), 0.0);
    }

    #[test]
    fn steady_movement_settles_at_its_speed() {
        let mut m = make(1);
        let (_, out) = ramp(&mut m, 0.0, 2.0, 1.0);
        assert!((out - 2.0).abs() < 0.01, "got {out}");
    }

    #[test]
    fn direction_is_ignored() {
        let mut up = make(1);
        let mut down = make(1);
        let (_, a) = ramp(&mut up, 0.0, 2.0, 1.0);
        let (_, b) = ramp(&mut down, 0.0, -2.0, 1.0);
        assert!((a - b).abs() < 1e-3, "{a} vs {b}");
    }

    #[test]
    fn stopping_decays_over_release_time() {
        let mut m = make(1);
        m.params.smooth = Some(volts(0.0));
        m.params.attack = Some(volts(0.0));
        m.params.release = Some(volts(0.5));
        let (x, level) = ramp(&mut m, 0.0, 2.0, 0.5);
        let after = hold(&mut m, x, 0.5);
        let expected = level * (-1.0f32).exp();
        assert!(
            (after - expected).abs() < 0.01,
            "got {after}, expected {expected}"
        );
    }

    #[test]
    fn attack_time_governs_rise() {
        let mut m = make(1);
        m.params.smooth = Some(volts(0.0));
        m.params.attack = Some(volts(0.2));
        let (_, out) = ramp(&mut m, 0.0, 2.0, 0.2);
        let expected = 2.0 * (1.0 - (-1.0f32).exp());
        assert!(
            (out - expected).abs() < 0.02,
            "got {out}, expected {expected}"
        );
    }

    #[test]
    fn stepped_controller_yields_steady_level_at_mean_speed() {
        // 7-bit CC over 0–5 V, one step every 5 ms: mean speed ≈ 7.87 V/s.
        let step_v = 5.0 / 127.0;
        let step_samples = (0.005 * SR) as usize;
        let mean_speed = step_v / 0.005;

        let mut m = make(1);
        m.params.smooth = Some(volts(0.05));
        let mut x = 0.0;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for i in 0..(2.0 * SR) as usize {
            if i % step_samples == 0 {
                x += step_v;
            }
            let out = step(&mut m, x);
            if i as f32 > 1.5 * SR {
                lo = lo.min(out);
                hi = hi.max(out);
            }
        }
        let mid = (lo + hi) / 2.0;
        assert!((mid - mean_speed).abs() < mean_speed * 0.1, "mid {mid}");
        assert!(hi - lo < mean_speed * 0.2, "ripple {lo}..{hi}");
    }

    #[test]
    fn output_stays_below_ceiling() {
        let mut m = make(1);
        m.params.ceiling = Some(volts(5.0));
        let (_, out) = ramp(&mut m, 0.0, 500.0, 0.2);
        assert!(out > 4.5 && out <= 5.0, "got {out}");
    }

    #[test]
    fn channels_are_independent() {
        let mut m = make(2);
        for i in 0..SR as usize {
            let t = i as f32 / SR;
            m.params.input = PolySignal::poly(&[Signal::Volts(2.0 * t), Signal::Volts(1.0)]);
            m.update(SR);
        }
        let a = m.outputs.sample.get(0);
        let b = m.outputs.sample.get(1);
        assert!((a - 2.0).abs() < 0.01, "got {a}");
        assert_eq!(b, 0.0);
    }

    #[test]
    fn nan_input_recovers() {
        let mut m = make(1);
        ramp(&mut m, 0.0, 2.0, 0.1);
        assert!(step(&mut m, f32::NAN).is_finite());
        let after = hold(&mut m, 1.0, 5.0);
        assert!(after.abs() < 1e-3, "got {after}");
    }

    #[test]
    fn pos_and_neg_follow_one_direction_each() {
        let mut up = make(1);
        let (_, main_up) = ramp(&mut up, 0.0, 2.0, 1.0);
        assert!((up.outputs.pos.get(0) - main_up).abs() < 1e-4);
        assert_eq!(up.outputs.neg.get(0), 0.0);

        let mut down = make(1);
        let (_, main_down) = ramp(&mut down, 0.0, -2.0, 1.0);
        assert!((down.outputs.neg.get(0) - main_down).abs() < 1e-4);
        assert_eq!(down.outputs.pos.get(0), 0.0);
    }

    #[test]
    fn pos_releases_while_neg_rises_on_reversal() {
        let mut m = make(1);
        let (x, _) = ramp(&mut m, 0.0, 2.0, 1.0);
        ramp(&mut m, x, -2.0, 1.0);
        let pos = m.outputs.pos.get(0);
        let neg = m.outputs.neg.get(0);
        assert!(pos < 0.1, "pos {pos}");
        assert!((neg - 2.0).abs() < 0.01, "neg {neg}");
    }
}
