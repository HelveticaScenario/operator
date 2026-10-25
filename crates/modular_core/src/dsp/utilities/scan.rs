use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::SchmittTrigger;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ScanParams {
    /// span the counter wraps within; must be above 0
    pub length: PolySignal,
    /// amount added to the counter on each trigger; negative steps backwards
    pub distance: PolySignal,
    /// rising edges step the counter by `distance`
    #[signal(type = trig, range = (0.0, 5.0))]
    pub trigger: PolySignal,
    /// rising edges set the counter to 0
    #[signal(type = trig, range = (0.0, 5.0))]
    #[deserr(default)]
    pub reset: Option<PolySignal>,
    /// a step past either end lands on the opposite end instead of wrapping
    /// by the overshoot (default false)
    #[serde(default)]
    #[deserr(default)]
    pub snap_on_wrap: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ScanOutputs {
    #[output("output", "the counter, from 0 to length", default)]
    pub output: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    counter: f32,
    trigger_schmitt: SchmittTrigger,
    reset_schmitt: SchmittTrigger,
}

/// Steps a counter by `distance` on each trigger, wrapping it within 0 to
/// `length`.
///
/// A step past `length` wraps around by the overshoot, as many times as it
/// takes, and so does a step below 0. With `snapOnWrap`, a step past
/// `length` lands on 0 and a step below 0 lands on `length`. A counter
/// exactly at `length` stays there. `reset` sets the counter to 0, and wins
/// over a trigger in the same sample. Lengths are read at each trigger, so a
/// shorter `length` takes effect on the next step.
///
/// ```js
/// // Pitch steps of a quarter octave, wrapping within an octave:
/// $sine($scan(1, 0.25, $pulse('4hz'))).out()
/// ```
#[module(name = "$scan", args(length, distance, trigger))]
pub struct Scan {
    params: ScanParams,
    outputs: ScanOutputs,
    channel_state: Box<[ChannelState]>,
}

message_handlers!(impl Scan {});

/// `counter` stepped by `distance` and brought back within 0 to `length`.
fn step(counter: f32, distance: f32, length: f32, snap_on_wrap: bool) -> f32 {
    // A non-positive or NaN length counts as the smallest positive one.
    let length = if length > 0.0 {
        length
    } else {
        f32::MIN_POSITIVE
    };
    let c = counter + if distance.is_finite() { distance } else { 0.0 };
    if c > length {
        if snap_on_wrap {
            return 0.0;
        }
        // Subtracting `length` until the counter is at most `length` leaves
        // it in (0, length]; the corrections catch a quotient rounded across
        // a whole number.
        let mut r = c - length * ((c / length).ceil() - 1.0);
        if r > length {
            r -= length;
        }
        if r <= 0.0 {
            r += length;
        }
        r.clamp(0.0, length)
    } else if c < 0.0 {
        if snap_on_wrap {
            return length;
        }
        // Adding `length` until the counter is at least 0 leaves it in
        // [0, length).
        let mut r = c + length * (-c / length).ceil();
        if r >= length {
            r -= length;
        }
        if r < 0.0 {
            r += length;
        }
        r.clamp(0.0, length)
    } else {
        c
    }
}

impl Scan {
    fn update(&mut self, _sample_rate: f32) {
        let p = &self.params;
        for ch in 0..self.channel_count() {
            let state = &mut self.channel_state[ch];
            if state.trigger_schmitt.process(p.trigger.get_value(ch)) {
                state.counter = step(
                    state.counter,
                    p.distance.get_value(ch),
                    p.length.get_value(ch),
                    p.snap_on_wrap,
                );
            }
            if state.reset_schmitt.process(p.reset.value_or_zero(ch)) {
                state.counter = 0.0;
            }
            self.outputs.output.set(ch, state.counter);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    fn make(length: f32, distance: f32, snap_on_wrap: bool) -> Scan {
        let mut outputs = ScanOutputs::default();
        outputs.set_all_channels(1);
        Scan {
            params: ScanParams {
                length: PolySignal::mono(Signal::Volts(length)),
                distance: PolySignal::mono(Signal::Volts(distance)),
                trigger: PolySignal::mono(Signal::Volts(0.0)),
                reset: None,
                snap_on_wrap,
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
        }
    }

    /// Fire `n` triggers and return the counter after each.
    fn fire(m: &mut Scan, n: usize) -> Vec<f32> {
        (0..n)
            .map(|_| {
                m.params.trigger = PolySignal::mono(Signal::Volts(5.0));
                m.update(48_000.0);
                m.params.trigger = PolySignal::mono(Signal::Volts(0.0));
                m.update(48_000.0);
                m.outputs.output.get(0)
            })
            .collect()
    }

    /// The counter rule as specified: step, then wrap one length at a time.
    fn step_by_loop(counter: f32, distance: f32, length: f32, snap_on_wrap: bool) -> f32 {
        let mut c = counter + distance;
        if snap_on_wrap {
            if c > length {
                c = 0.0;
            } else if c < 0.0 {
                c = length;
            }
            return c;
        }
        while c > length {
            c -= length;
        }
        while c < 0.0 {
            c += length;
        }
        c
    }

    #[test]
    fn steps_wrap_by_their_overshoot() {
        let mut m = make(4.0, 3.0, false);
        assert_eq!(fire(&mut m, 5), vec![3.0, 2.0, 1.0, 4.0, 3.0]);
    }

    #[test]
    fn negative_steps_wrap_below_zero() {
        let mut m = make(4.0, -3.0, false);
        assert_eq!(fire(&mut m, 5), vec![1.0, 2.0, 3.0, 0.0, 1.0]);
    }

    #[test]
    fn steps_longer_than_length_wrap_repeatedly() {
        let mut m = make(4.0, 10.5, false);
        assert_eq!(fire(&mut m, 2), vec![2.5, 1.0]);
        let mut m = make(4.0, -10.5, false);
        assert_eq!(fire(&mut m, 2), vec![1.5, 3.0]);
    }

    #[test]
    fn snap_on_wrap_lands_on_the_opposite_end() {
        let mut m = make(4.0, 3.0, true);
        assert_eq!(fire(&mut m, 4), vec![3.0, 0.0, 3.0, 0.0]);
        let mut m = make(4.0, -3.0, true);
        assert_eq!(fire(&mut m, 4), vec![4.0, 1.0, 4.0, 1.0]);
    }

    #[test]
    fn reset_wins_over_a_trigger_in_the_same_sample() {
        let mut m = make(4.0, 3.0, false);
        fire(&mut m, 1);
        m.params.trigger = PolySignal::mono(Signal::Volts(5.0));
        m.params.reset = Some(PolySignal::mono(Signal::Volts(5.0)));
        m.update(48_000.0);
        assert_eq!(m.outputs.output.get(0), 0.0);
    }

    #[test]
    fn matches_wrapping_one_length_at_a_time() {
        let mut rng = crate::dsp::utils::rng::LcgRng::default();
        rng.seed(7, 0);
        for snap in [false, true] {
            for _ in 0..100_000 {
                let length = 0.1 + rng.next_unit() * 10.0;
                let counter = rng.next_unit() * length;
                let distance = (rng.next_unit() - 0.5) * 100.0;
                let expected = step_by_loop(counter, distance, length, snap);
                let actual = step(counter, distance, length, snap);
                assert!((0.0..=length).contains(&actual));
                // The loop rounds at every subtraction, so compare positions
                // around the cycle, where 0 and `length` meet.
                let d = (actual - expected).abs();
                assert!(
                    d.min(length - d) <= 1e-4 * length.max(distance.abs()),
                    "{counter} + {distance} in {length}: {actual} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn invalid_values_keep_the_counter_in_range() {
        for length in [0.0, -1.0, f32::NAN] {
            let c = step(0.0, 3.0, length, false);
            assert!((0.0..=f32::MIN_POSITIVE).contains(&c), "{length}: {c}");
        }
        assert_eq!(step(1.0, f32::NAN, 4.0, false), 1.0);
        assert_eq!(step(1.0, f32::INFINITY, 4.0, false), 1.0);
    }

    #[test]
    fn channels_count_independently() {
        let mut outputs = ScanOutputs::default();
        outputs.set_all_channels(2);
        let mut m = Scan {
            params: ScanParams {
                length: PolySignal::mono(Signal::Volts(4.0)),
                distance: PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(-1.0)]),
                trigger: PolySignal::mono(Signal::Volts(5.0)),
                reset: None,
                snap_on_wrap: false,
            },
            outputs,
            _channel_count: 2,
            _block_index: Default::default(),
            channel_state: vec![ChannelState::default(); 2].into_boxed_slice(),
        };
        m.update(48_000.0);
        assert_eq!(m.outputs.output.get(0), 1.0);
        assert_eq!(m.outputs.output.get(1), 3.0);
    }
}
