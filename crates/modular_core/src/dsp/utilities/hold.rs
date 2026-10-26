use crate::dsp::utils::{GATE_HIGH_VOLTAGE, SchmittTrigger, min_gate_samples};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct HoldParams {
    /// gate/trigger input — a rising edge opens the output
    #[signal(type = gate, range = (0.0, 5.0))]
    input: PolySignal,
    /// hold time in seconds (default 0.05)
    #[signal(default = 0.05, range = (0.0, 10.0))]
    #[deserr(default)]
    time: Option<PolySignal>,
    /// when true, a rising edge mid-hold restarts the gate; when false, it is
    /// ignored and the gate closes `time` after the edge that opened it
    /// (default true)
    #[serde(default = "default_true")]
    #[deserr(default = default_true())]
    retrigger: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct HoldOutputs {
    #[output("output", "held gate", default, range = (0.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Clone, Copy, Default)]
struct HoldChannelState {
    schmitt: SchmittTrigger,
    elapsed: f32,
    active: bool,
    /// Samples left in a retrigger gap, during which the output is held low.
    gap_remaining: u32,
    /// Whether the previous sample's output was high: a rising edge then needs
    /// a gap for downstream gate inputs to see a new edge.
    output_high: bool,
}

/// Stretches a trigger into a fixed-length gate.
///
/// A rising edge on the input opens the output at 5V for `time` seconds,
/// regardless of when the input falls; the output closes after `time` even if
/// the input is still high. A rising edge while the output is high
/// retriggers it: the output drops low for a brief retrigger gap, so
/// downstream envelopes see a new rising edge, and the gate closes `time`
/// after the new edge.
///
/// With `retrigger: false`, edges are ignored while the output is high, so
/// the gate re-opens on the first edge after it closes. Fed a steady pulse
/// train, it fires once every `ceil(time / period)` pulses: a hold longer
/// than the period divides the rate into subharmonics.
///
/// ## Example
///
/// ```js
/// // stretch short clock pulses into 100 ms gates
/// const env = $adsr($hold($clock.beatTrigger, 0.1))
/// $sine('c4').amplitude(env).out()
/// ```
///
/// ```js
/// // a 12 ms hold on a 200 Hz pulse train fires every third pulse
/// $hold($pulse('200hz'), 0.012, { retrigger: false }).out()
/// ```
#[module(name = "$hold", args(input, time))]
pub struct Hold {
    outputs: HoldOutputs,
    params: HoldParams,
    channel_state: Box<[HoldChannelState]>,
}

impl Hold {
    fn update(&mut self, sample_rate: f32) {
        let sample_period = 1.0 / sample_rate;
        let retrigger_gap = min_gate_samples(sample_rate);
        for ch in 0..self.channel_count() {
            let state = &mut self.channel_state[ch];
            let time = self.params.time.value_or(ch, 0.05).max(0.0);
            let (_, edge) = state
                .schmitt
                .process_with_edge(self.params.input.get_value(ch));
            let holding = state.active && state.elapsed < time;
            if edge.is_rising() && (self.params.retrigger || !holding) {
                if state.output_high {
                    state.gap_remaining = retrigger_gap;
                }
                state.elapsed = 0.0;
                state.active = true;
            }
            if state.active && state.elapsed < time {
                state.elapsed += sample_period;
                state.output_high = state.gap_remaining == 0;
                if state.gap_remaining > 0 {
                    state.gap_remaining -= 1;
                }
            } else {
                state.active = false;
                state.output_high = false;
            }
            self.outputs.sample.set(
                ch,
                if state.output_high {
                    GATE_HIGH_VOLTAGE
                } else {
                    0.0
                },
            );
        }
    }
}

message_handlers!(impl Hold {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48_000.0;

    fn make(input_volts: f32, time_secs: Option<f32>) -> Hold {
        let mut outputs = HoldOutputs::default();
        outputs.set_all_channels(1);
        Hold {
            params: HoldParams {
                input: PolySignal::mono(Signal::Volts(input_volts)),
                time: time_secs.map(|t| PolySignal::mono(Signal::Volts(t))),
                retrigger: true,
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            channel_state: vec![HoldChannelState::default(); 1].into_boxed_slice(),
        }
    }

    fn set_input(hold: &mut Hold, volts: f32) {
        hold.params.input = PolySignal::mono(Signal::Volts(volts));
    }

    /// Run `n` samples, returning how many produced a high (5V) output.
    fn count_high(hold: &mut Hold, n: usize) -> usize {
        let mut high = 0;
        for _ in 0..n {
            hold.update(SR);
            if hold.outputs.sample.get(0) >= GATE_HIGH_VOLTAGE - 0.01 {
                high += 1;
            }
        }
        high
    }

    fn assert_close(actual: usize, expected: usize) {
        assert!(
            actual.abs_diff(expected) <= 2,
            "expected ~{expected} high samples, got {actual}"
        );
    }

    #[test]
    fn rising_edge_opens_gate_for_time() {
        let mut hold = make(0.0, Some(0.01));
        assert_eq!(count_high(&mut hold, 8), 0);
        set_input(&mut hold, 5.0);
        assert_close(count_high(&mut hold, 1000), 480);
    }

    #[test]
    fn output_low_after_hold_even_while_input_high() {
        let mut hold = make(0.0, Some(0.01));
        set_input(&mut hold, 5.0);
        count_high(&mut hold, 1000);
        assert_eq!(count_high(&mut hold, 100), 0);
    }

    /// Run `n` samples, returning the output of each.
    fn outputs(hold: &mut Hold, n: usize) -> Vec<f32> {
        (0..n)
            .map(|_| {
                hold.update(SR);
                hold.outputs.sample.get(0)
            })
            .collect()
    }

    #[test]
    fn retrigger_mid_hold_drops_low_for_a_gap_then_restarts_the_timer() {
        let mut hold = make(0.0, Some(0.01));
        set_input(&mut hold, 5.0);
        count_high(&mut hold, 100);
        set_input(&mut hold, 0.0);
        count_high(&mut hold, 10);
        set_input(&mut hold, 5.0);
        let out = outputs(&mut hold, 1000);
        let gap = min_gate_samples(SR) as usize;
        // The output falls so downstream gate inputs see a new rising edge…
        assert!(out[..gap].iter().all(|&v| v == 0.0), "{:?}", &out[..gap]);
        assert_eq!(out[gap], GATE_HIGH_VOLTAGE);
        // …and the gate still closes `time` after the new edge.
        let high = out
            .iter()
            .filter(|&&v| v >= GATE_HIGH_VOLTAGE - 0.01)
            .count();
        assert_close(high, 480 - gap);
    }

    /// A 1-sample trigger followed by `low` samples of low input.
    fn pulse_then_low(low: usize) -> (Hold, Vec<f32>) {
        let mut hold = make(0.0, Some(0.01));
        set_input(&mut hold, 5.0);
        let mut out = outputs(&mut hold, 1);
        set_input(&mut hold, 0.0);
        out.extend(outputs(&mut hold, low));
        (hold, out)
    }

    #[test]
    fn a_trigger_on_the_sample_a_hold_would_close_still_gets_a_gap() {
        let (_, out) = pulse_then_low(1000);
        let gate_len = out.iter().take_while(|&&v| v == GATE_HIGH_VOLTAGE).count();

        // Retrigger on sample `gate_len`: the first one the gate would be low.
        let (mut hold, _) = pulse_then_low(gate_len - 1);
        set_input(&mut hold, 5.0);
        let gap = min_gate_samples(SR) as usize;
        let after = outputs(&mut hold, gap + 1);
        assert!(after[..gap].iter().all(|&v| v == 0.0), "{after:?}");
        assert_eq!(after[gap], GATE_HIGH_VOLTAGE);
    }

    #[test]
    fn retrigger_after_hold_ends_has_no_gap() {
        let mut hold = make(0.0, Some(0.01));
        set_input(&mut hold, 5.0);
        count_high(&mut hold, 10);
        set_input(&mut hold, 0.0);
        count_high(&mut hold, 1000);
        set_input(&mut hold, 5.0);
        assert_eq!(outputs(&mut hold, 1)[0], GATE_HIGH_VOLTAGE);
    }

    #[test]
    fn short_trigger_yields_full_gate() {
        let mut hold = make(0.0, Some(0.01));
        set_input(&mut hold, 5.0);
        let head = count_high(&mut hold, 3);
        set_input(&mut hold, 0.0);
        assert_close(head + count_high(&mut hold, 1000), 480);
    }

    #[test]
    fn default_time_holds_50ms() {
        let mut hold = make(0.0, None);
        set_input(&mut hold, 5.0);
        assert_close(count_high(&mut hold, 5000), 2400);
    }

    /// Rising edges of the output while `hold` is fed a pulse train with
    /// `period` samples and a 1-sample high, over `n` samples.
    fn output_edges(hold: &mut Hold, period: usize, n: usize) -> usize {
        let mut edges = 0;
        let mut prev = 0.0;
        for i in 0..n {
            set_input(hold, if i % period == 0 { 5.0 } else { 0.0 });
            hold.update(SR);
            let v = hold.outputs.sample.get(0);
            if v > prev {
                edges += 1;
            }
            prev = v;
        }
        edges
    }

    #[test]
    fn without_retrigger_a_mid_hold_edge_is_ignored() {
        let mut hold = make(0.0, Some(0.01));
        hold.params.retrigger = false;
        set_input(&mut hold, 5.0);
        count_high(&mut hold, 100);
        set_input(&mut hold, 0.0);
        count_high(&mut hold, 10);
        set_input(&mut hold, 5.0);
        let out = outputs(&mut hold, 1000);
        // No gap, and the gate closes `time` after the first edge.
        let high = out.iter().take_while(|&&v| v == GATE_HIGH_VOLTAGE).count();
        assert_close(high, 480 - 110);
        assert!(out[high..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn without_retrigger_a_hold_longer_than_the_period_divides_the_rate() {
        // 100-sample period; the gate re-opens on the first pulse after it
        // closes, so a hold of k periods plus a bit fires every k + 1 pulses.
        for (time_samples, division) in [(50.0, 1), (150.0, 2), (250.0, 3), (390.0, 4)] {
            let mut hold = make(0.0, Some(time_samples / SR));
            hold.params.retrigger = false;
            let edges = output_edges(&mut hold, 100, 100 * 120);
            assert_eq!(edges, 120 / division, "time {time_samples} samples");
        }
    }

    #[test]
    fn without_retrigger_an_edge_on_the_closing_sample_reopens_after_a_gap() {
        let (_, out) = pulse_then_low(1000);
        let gate_len = out.iter().take_while(|&&v| v == GATE_HIGH_VOLTAGE).count();

        let (mut hold, _) = pulse_then_low(gate_len - 1);
        hold.params.retrigger = false;
        set_input(&mut hold, 5.0);
        let gap = min_gate_samples(SR) as usize;
        let after = outputs(&mut hold, gap + 1);
        assert!(after[..gap].iter().all(|&v| v == 0.0), "{after:?}");
        assert_eq!(after[gap], GATE_HIGH_VOLTAGE);
    }

    #[test]
    fn with_retrigger_a_hold_longer_than_the_period_keeps_every_pulse() {
        let mut hold = make(0.0, Some(250.0 / SR));
        assert_eq!(output_edges(&mut hold, 100, 100 * 120), 120);
    }
}
