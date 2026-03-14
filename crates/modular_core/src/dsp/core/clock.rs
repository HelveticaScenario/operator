use napi::Result;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::PolyOutput;
use crate::dsp::utils::{SchmittTrigger, TempGate, TempGateState, min_gate_samples};
use crate::types::ClockMessages;

/// Deserialize a u32 that must be >= 1 (positive integer).
/// Rejects 0 with a descriptive error so any clock instance gets validated.
fn deserialize_positive_u32<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = u32::deserialize(deserializer)?;
    if v == 0 {
        return Err(serde::de::Error::custom(
            "must be a positive integer (>= 1)",
        ));
    }
    Ok(v)
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct ClockParams {
    /// Tempo in BPM.
    tempo: f64,
    /// Time signature numerator (beats per bar). Must be a positive integer.
    #[serde(deserialize_with = "deserialize_positive_u32")]
    numerator: u32,
    /// Time signature denominator (beat value). Must be a positive integer.
    #[serde(deserialize_with = "deserialize_positive_u32")]
    denominator: u32,
}

struct ClockState {
    phase: f64,
    ppq_phase: f64,
    beat_phase: f64,
    /// Detects bar-boundary zero-crossings on (phase_increment − phase).
    bar_schmitt: SchmittTrigger,
    /// Detects PPQ-boundary zero-crossings.
    ppq_schmitt: SchmittTrigger,
    /// Detects beat-boundary zero-crossings.
    beat_schmitt: SchmittTrigger,
    /// Multi-sample bar trigger pulse.
    bar_gate: TempGate,
    /// Multi-sample PPQ trigger pulse.
    ppq_gate: TempGate,
    /// Multi-sample beat trigger pulse.
    beat_gate: TempGate,
    running: bool,
    loop_index: u64,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            phase: 0.0,
            ppq_phase: 0.0,
            beat_phase: 0.0,
            bar_schmitt: SchmittTrigger::new(0.0, 0.0),
            ppq_schmitt: SchmittTrigger::new(0.0, 0.0),
            beat_schmitt: SchmittTrigger::new(0.0, 0.0),
            bar_gate: TempGate::new_gate(TempGateState::Low),
            ppq_gate: TempGate::new_gate(TempGateState::Low),
            beat_gate: TempGate::new_gate(TempGateState::Low),
            running: true,
            loop_index: 0,
        }
    }
}

/// Tempo-synced transport clock for driving sequencers, envelopes, and synced modulation.
#[module(name = "_clock", channels = 2, args(tempo, numerator, denominator))]
pub struct Clock {
    outputs: ClockOutputs,
    state: ClockState,
    params: ClockParams,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClockOutputs {
    #[output(
        "playhead",
        "Bar playhead: channel 0 is bar phase (0..1), channel 1 is completed bar count",
        default
    )]
    playhead: PolyOutput,
    #[output("barTrigger", "5V trigger at the start of each bar", range = (0.0, 5.0))]
    bar_trigger: f32,
    #[output("beatTrigger", "5V trigger at the start of each beat", range = (0.0, 5.0))]
    beat_trigger: f32,
    #[output("ramp", "0..5V ramp that resets every bar", range = (0.0, 5.0))]
    ramp: f32,
    #[output("ppqTrigger", "5V trigger at 48 pulses per quarter note", range = (0.0, 5.0))]
    ppq_trigger: f32,
    #[output("beatInBar", "Current beat within the bar (0-indexed)")]
    beat_in_bar: f32,
}

message_handlers!(impl Clock {
    Clock(m) => Clock::on_clock_message,
});

impl Clock {
    fn update(&mut self, sample_rate: f32) {
        if !self.state.running {
            return;
        }

        let bpm = self.params.tempo.max(1.0);
        let frequency_hz = bpm / 60.0;

        let numerator = self.params.numerator.max(1) as f64;
        let denominator = self.params.denominator.max(1) as f64;

        let quarter_notes_per_bar = numerator * 4.0 / denominator;
        let bar_frequency = frequency_hz / quarter_notes_per_bar;
        let phase_increment = bar_frequency / sample_rate as f64;

        self.state.phase += phase_increment;
        self.state.ppq_phase += phase_increment;
        self.state.beat_phase += phase_increment;

        if self.state.phase >= 1.0 {
            self.state.phase -= 1.0;
            self.state.loop_index += 1;
        }

        let ppq_period = 1.0 / (12.0 * quarter_notes_per_bar);
        if self.state.ppq_phase >= ppq_period {
            self.state.ppq_phase -= ppq_period;
        }

        let beat_period = 1.0 / numerator;
        if self.state.beat_phase >= beat_period {
            self.state.beat_phase -= beat_period;
        }

        self.outputs.beat_in_bar = (self.state.phase * numerator).floor() as f32;

        self.outputs.playhead.set(0, self.state.phase as f32);
        self.outputs.playhead.set(1, self.state.loop_index as f32);

        self.outputs.ramp = self.state.phase as f32 * 5.0;

        let hold = min_gate_samples(sample_rate);

        if self
            .state
            .bar_schmitt
            .process((phase_increment - self.state.phase) as f32)
        {
            self.state
                .bar_gate
                .set_state(TempGateState::High, TempGateState::Low, hold);
        }
        self.outputs.bar_trigger = self.state.bar_gate.process();

        if self
            .state
            .beat_schmitt
            .process((phase_increment - self.state.beat_phase) as f32)
        {
            self.state
                .beat_gate
                .set_state(TempGateState::High, TempGateState::Low, hold);
        }
        self.outputs.beat_trigger = self.state.beat_gate.process();

        if self
            .state
            .ppq_schmitt
            .process((phase_increment - self.state.ppq_phase) as f32)
        {
            self.state
                .ppq_gate
                .set_state(TempGateState::High, TempGateState::Low, hold);
        }
        self.outputs.ppq_trigger = self.state.ppq_gate.process();
    }

    fn on_clock_message(&mut self, m: &ClockMessages) -> Result<()> {
        match m {
            ClockMessages::Start => {
                self.state.running = true;
                self.state.phase = 0.0;
                self.state.ppq_phase = 0.0;
                self.state.beat_phase = 0.0;
                self.outputs.playhead.set(0, 0.0);
                self.outputs.playhead.set(1, 0.0);
                self.state.loop_index = 0;
                self.outputs.beat_in_bar = 0.0;
                self.state.bar_schmitt.reset();
                self.state.ppq_schmitt.reset();
                self.state.beat_schmitt.reset();
                self.state
                    .bar_gate
                    .set_state(TempGateState::Low, TempGateState::Low, 0);
                self.state
                    .ppq_gate
                    .set_state(TempGateState::Low, TempGateState::Low, 0);
                self.state
                    .beat_gate
                    .set_state(TempGateState::Low, TempGateState::Low, 0);
            }
            ClockMessages::Stop => {
                self.state.running = false;
                println!("Clock stopped");
                self.outputs.bar_trigger = 0.0;
                self.outputs.beat_trigger = 0.0;
                self.outputs.ppq_trigger = 0.0;
                self.outputs.playhead.set(0, 0.0);
                self.outputs.playhead.set(1, 0.0);
                self.state.loop_index = 0;
                self.outputs.beat_in_bar = 0.0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_start_stop_via_message() {
        let mut c = Clock::default();
        let sr = 48_000.0;

        let _ = c.on_clock_message(&ClockMessages::Stop);
        let phase_before = c.state.phase;
        for _ in 0..128 {
            c.update(sr);
        }
        assert!((c.state.phase - phase_before).abs() < 1e-9);

        let _ = c.on_clock_message(&ClockMessages::Start);
        assert!((c.state.phase - 0.0).abs() < 1e-9);

        c.update(sr);
        assert!(c.state.phase > 0.0);
    }

    /// Helper: count how many trigger events (rising edges) beat_trigger has over a given number of samples
    fn count_beat_triggers(c: &mut Clock, sr: f32, samples: usize) -> usize {
        let mut count = 0;
        let mut was_high = false;
        for _ in 0..samples {
            c.update(sr);
            let is_high = c.outputs.beat_trigger == 5.0;
            if is_high && !was_high {
                count += 1;
            }
            was_high = is_high;
        }
        count
    }

    /// Helper: count how many trigger events (rising edges) bar_trigger has over a given number of samples
    fn count_bar_triggers(c: &mut Clock, sr: f32, samples: usize) -> usize {
        let mut count = 0;
        let mut was_high = false;
        for _ in 0..samples {
            c.update(sr);
            let is_high = c.outputs.bar_trigger == 5.0;
            if is_high && !was_high {
                count += 1;
            }
            was_high = is_high;
        }
        count
    }

    #[test]
    fn clock_default_time_sig_is_4_4() {
        let c = Clock::default();
        assert_eq!(c.params.numerator, 4);
        assert_eq!(c.params.denominator, 4);
    }

    #[test]
    fn clock_default_tempo_is_120() {
        let c = Clock::default();
        assert!((c.params.tempo - 120.0).abs() < 1e-9);
    }

    #[test]
    fn clock_beat_trigger_fires_4_times_per_bar_in_4_4() {
        let mut c = Clock::default();
        let sr = 48_000.0;
        // 120 BPM in 4/4 = one bar every 2 seconds = 96000 samples.
        // Use samples_per_bar - 1 to avoid counting the next bar's opening trigger.
        let samples = 96_000 - 1;

        let _ = c.on_clock_message(&ClockMessages::Start);
        let beats = count_beat_triggers(&mut c, sr, samples);
        assert_eq!(beats, 4, "4/4 time should produce 4 beat triggers per bar");
    }

    #[test]
    fn clock_beat_trigger_fires_3_times_per_bar_in_3_4() {
        let mut c = Clock::default();
        c.params.numerator = 3;
        c.params.denominator = 4;
        let sr = 48_000.0;
        // 120 BPM in 3/4 = 3 quarter notes per bar = 1.5 seconds = 72000 samples
        let samples = 72_000 - 1;

        let _ = c.on_clock_message(&ClockMessages::Start);
        let beats = count_beat_triggers(&mut c, sr, samples);
        assert_eq!(beats, 3, "3/4 time should produce 3 beat triggers per bar");
    }

    #[test]
    fn clock_beat_trigger_fires_6_times_per_bar_in_6_8() {
        let mut c = Clock::default();
        c.params.numerator = 6;
        c.params.denominator = 8;
        let sr = 48_000.0;
        // 120 BPM in 6/8 = 6 eighth notes per bar = 3 quarter notes = 1.5 seconds = 72000 samples
        let samples = 72_000 - 1;

        let _ = c.on_clock_message(&ClockMessages::Start);
        let beats = count_beat_triggers(&mut c, sr, samples);
        assert_eq!(beats, 6, "6/8 time should produce 6 beat triggers per bar");
    }

    #[test]
    fn clock_beat_trigger_fires_7_times_per_bar_in_7_8() {
        let mut c = Clock::default();
        c.params.numerator = 7;
        c.params.denominator = 8;
        let sr = 48_000.0;
        // 120 BPM in 7/8 = 7 eighth notes = 3.5 quarter notes = 1.75 seconds = 84000 samples
        let samples = 84_000 - 1;

        let _ = c.on_clock_message(&ClockMessages::Start);
        let beats = count_beat_triggers(&mut c, sr, samples);
        assert_eq!(beats, 7, "7/8 time should produce 7 beat triggers per bar");
    }

    #[test]
    fn clock_bar_trigger_count_matches_time_sig() {
        let sr = 48_000.0;
        // Run 4 bars worth at 120 BPM in 3/4 time
        // 3/4: 3 quarter notes per bar = 1.5s per bar, 4 bars = 6s = 288000 samples
        let mut c = Clock::default();
        c.params.numerator = 3;
        c.params.denominator = 4;
        let _ = c.on_clock_message(&ClockMessages::Start);

        let bar_triggers = count_bar_triggers(&mut c, sr, 288_000 - 1);
        assert_eq!(
            bar_triggers, 4,
            "Should produce 4 bar triggers over 4 bars in 3/4"
        );
    }

    #[test]
    fn clock_time_sig_deserialization() {
        // Verify time sig params deserialize correctly from JSON
        let params: ClockParams =
            serde_json::from_str(r#"{"numerator": 6, "denominator": 8}"#).unwrap();
        assert_eq!(params.numerator, 6);
        assert_eq!(params.denominator, 8);
    }

    #[test]
    fn clock_tempo_deserialization() {
        let params: ClockParams = serde_json::from_str(r#"{"tempo": 140}"#).unwrap();
        assert!((params.tempo - 140.0).abs() < 1e-9);
    }

    #[test]
    fn clock_time_sig_defaults_when_missing() {
        // Verify defaults when not provided in JSON
        let params: ClockParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.numerator, 4);
        assert_eq!(params.denominator, 4);
    }

    #[test]
    fn clock_beat_trigger_resets_on_start() {
        let mut c = Clock::default();
        let sr = 48_000.0;

        for _ in 0..24_000 {
            c.update(sr);
        }

        let _ = c.on_clock_message(&ClockMessages::Start);
        assert!(
            (c.state.beat_phase - 0.0).abs() < 1e-9,
            "beat_phase should be reset on Start"
        );
    }

    #[test]
    fn clock_stop_clears_beat_trigger() {
        let mut c = Clock::default();
        let sr = 48_000.0;

        c.update(sr);
        let _ = c.on_clock_message(&ClockMessages::Stop);
        assert_eq!(
            c.outputs.beat_trigger, 0.0,
            "beat_trigger should be 0 after Stop"
        );
    }

    #[test]
    fn clock_rejects_zero_numerator() {
        let result: std::result::Result<ClockParams, _> =
            serde_json::from_str(r#"{"numerator": 0}"#);
        assert!(result.is_err(), "numerator=0 should be rejected");
    }

    #[test]
    fn clock_rejects_zero_denominator() {
        let result: std::result::Result<ClockParams, _> =
            serde_json::from_str(r#"{"denominator": 0}"#);
        assert!(result.is_err(), "denominator=0 should be rejected");
    }

    #[test]
    fn clock_beat_in_bar_output() {
        let mut c = Clock::default();
        let sr = 48_000.0;
        // 120 BPM, 4/4 time: one bar = 2 seconds = 96000 samples
        // Each beat = 24000 samples

        let _ = c.on_clock_message(&ClockMessages::Start);

        // First sample: beat_in_bar should be 0
        c.update(sr);
        assert_eq!(c.outputs.beat_in_bar, 0.0, "First sample should be beat 0");

        // Advance to halfway through beat 1 (sample 24000+12000=36000)
        for _ in 1..36_000 {
            c.update(sr);
        }
        assert_eq!(
            c.outputs.beat_in_bar, 1.0,
            "Should be on beat 1 at 36000 samples"
        );

        // Advance to beat 2 area
        for _ in 0..24_000 {
            c.update(sr);
        }
        assert_eq!(
            c.outputs.beat_in_bar, 2.0,
            "Should be on beat 2 at 60000 samples"
        );

        // Advance to beat 3 area
        for _ in 0..24_000 {
            c.update(sr);
        }
        assert_eq!(
            c.outputs.beat_in_bar, 3.0,
            "Should be on beat 3 at 84000 samples"
        );
    }
}
