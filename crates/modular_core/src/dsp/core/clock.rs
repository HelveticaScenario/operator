use deserr::Deserr;
use napi::Result;
use schemars::JsonSchema;

use crate::PolyOutput;
use crate::dsp::utils::{SchmittTrigger, TempGate, TempGateState, min_gate_samples};
use crate::types::ClockMessages;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ClockParams {
    /// Tempo in BPM.
    tempo: f64,
    /// Time signature numerator (beats per bar). Must be a positive integer.
    numerator: u32,
    /// Time signature denominator (beat value). Must be a positive integer.
    denominator: u32,
}

#[derive(Clone, Copy)]
struct ExternalClockSync {
    bar_phase: f64,
    bpm: f64,
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
    external_sync: Option<ExternalClockSync>,
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
            external_sync: None,
        }
    }
}

/// Tempo-synced transport clock for driving sequencers, envelopes, and synced modulation.
#[module(
    name = "_clock",
    channels = 2,
    args(tempo, numerator, denominator),
    clock_sync
)]
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
    pub fn sync_external_clock(&mut self, bar_phase: f64, bpm: f64) {
        self.state.external_sync = Some(ExternalClockSync { bar_phase, bpm });
    }

    pub fn clear_external_sync(&mut self) {
        self.state.external_sync = None;
    }

    fn update(&mut self, sample_rate: f32) {
        // External clock sync: override free-running clock with Link data.
        // Whether the clock produces output is gated by the enclosing
        // Operator `stopped` flag, not by the peer's playing state.
        if let Some(sync) = self.state.external_sync.take() {
            self.state.running = true;
            self.params.tempo = sync.bpm;

            let numerator = self.params.numerator.max(1) as f64;
            let denominator = self.params.denominator.max(1) as f64;

            let old_phase = self.state.phase;
            self.state.phase = sync.bar_phase;
            self.state.loop_index = if sync.bar_phase < old_phase && old_phase > 0.5 {
                self.state.loop_index + 1
            } else {
                self.state.loop_index
            };

            // Derive beat and PPQ phases from bar phase
            let beat_period = 1.0 / numerator;
            self.state.beat_phase = sync.bar_phase % beat_period;

            let quarter_notes_per_bar = numerator * 4.0 / denominator;
            let ppq_period = 1.0 / (12.0 * quarter_notes_per_bar);
            self.state.ppq_phase = sync.bar_phase % ppq_period;

            // Detect bar boundary crossing
            let hold = min_gate_samples(sample_rate);
            if sync.bar_phase < old_phase && old_phase > 0.5 {
                self.state
                    .bar_gate
                    .set_state(TempGateState::High, TempGateState::Low, hold);
            }
            self.outputs.bar_trigger = self.state.bar_gate.process();

            // Detect beat boundary crossing
            let old_beat = (old_phase / beat_period).floor();
            let new_beat = (sync.bar_phase / beat_period).floor();
            let first_sync_frame = old_phase == 0.0 && self.state.loop_index == 0;
            if new_beat != old_beat
                || (sync.bar_phase < old_phase && old_phase > 0.5)
                || first_sync_frame
            {
                self.state
                    .beat_gate
                    .set_state(TempGateState::High, TempGateState::Low, hold);
            }
            self.outputs.beat_trigger = self.state.beat_gate.process();

            // Detect PPQ boundary crossing
            let old_ppq = (old_phase / ppq_period).floor();
            let new_ppq = (sync.bar_phase / ppq_period).floor();
            if new_ppq != old_ppq || (sync.bar_phase < old_phase && old_phase > 0.5) {
                self.state
                    .ppq_gate
                    .set_state(TempGateState::High, TempGateState::Low, hold);
            }
            self.outputs.ppq_trigger = self.state.ppq_gate.process();

            // Update remaining outputs
            self.outputs.beat_in_bar = (sync.bar_phase * numerator).floor() as f32;
            self.outputs.playhead.set(0, sync.bar_phase as f32);
            self.outputs.playhead.set(1, self.state.loop_index as f32);
            self.outputs.ramp = sync.bar_phase as f32 * 5.0;

            return;
        }

        if !self.state.running {
            return; // If not running, skip the rest of the update to keep outputs where they are until clock starts
        }

        // Tempo is a plain BPM value
        let bpm = self.params.tempo.max(1.0);
        let frequency_hz = bpm / 60.0;

        // Time signature: numerator = beats per bar, denominator = beat value
        // Clamp to valid values (minimum 1) to avoid division by zero
        let numerator = self.params.numerator.max(1) as f64;
        let denominator = self.params.denominator.max(1) as f64;

        // Calculate phase increment per sample
        // BPM tempo is in quarter notes per minute, so frequency_hz = quarter notes per second.
        // quarter_notes_per_bar tells us how many quarter notes fit in one bar given the time sig.
        // e.g. 4/4 = 4 quarter notes, 3/4 = 3, 6/8 = 3, 7/8 = 3.5
        let quarter_notes_per_bar = numerator * 4.0 / denominator;
        let bar_frequency = frequency_hz / quarter_notes_per_bar;
        let phase_increment = bar_frequency / sample_rate as f64;

        self.state.phase += phase_increment;
        self.state.ppq_phase += phase_increment;
        self.state.beat_phase += phase_increment;

        // Wrap phase at 1.0
        if self.state.phase >= 1.0 {
            self.state.phase -= 1.0;
            self.state.loop_index += 1;
        }

        // PPQ phase wraps at 12 PPQ per quarter note (= 12 * quarter_notes_per_bar per bar)
        let ppq_period = 1.0 / (12.0 * quarter_notes_per_bar);
        if self.state.ppq_phase >= ppq_period {
            self.state.ppq_phase -= ppq_period;
        }

        // Beat phase wraps once per beat (numerator beats per bar)
        let beat_period = 1.0 / numerator;
        if self.state.beat_phase >= beat_period {
            self.state.beat_phase -= beat_period;
        }

        // Derive beat_in_bar from the bar phase
        // phase goes from 0..1 over one bar, each beat occupies 1/numerator of the bar
        self.outputs.beat_in_bar = (self.state.phase * numerator).floor() as f32;

        self.outputs.playhead.set(0, self.state.phase as f32);
        self.outputs.playhead.set(1, self.state.loop_index as f32);

        // Generate ramp output (0 to 5V over one bar)
        self.outputs.ramp = self.state.phase as f32 * 5.0;

        let hold = min_gate_samples(sample_rate);

        // --- Trigger generation via SchmittTrigger + TempGate ---
        //
        // For each phase (bar, beat, ppq) the signal `phase_increment − phase`
        // is negative for most of the cycle and goes positive at the wrap
        // point (when phase resets near zero). A SchmittTrigger with both
        // thresholds at 0.0 detects this rising edge, and a TempGate
        // stretches the single-sample event into a multi-sample 5V pulse
        // of duration `hold` (≈16 samples at 48 kHz).

        // Bar trigger
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

        // Beat trigger
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

        // PPQ trigger
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
                // Start implies a transport reset.
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
                // Ensure triggers are low while stopped.
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
impl Default for ClockParams {
    fn default() -> Self {
        Self {
            tempo: 120.0,
            numerator: 4,
            denominator: 4,
        }
    }
}

#[cfg(test)]
impl Default for Clock {
    fn default() -> Self {
        Self {
            outputs: ClockOutputs::default(),
            state: ClockState::default(),
            params: ClockParams::default(),
            _channel_count: 2,
        }
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

        // Start should reset and run.
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

    /// Helper to deserialize ClockParams from a serde_json::Value via deserr.
    fn deserialize_clock_params(json: serde_json::Value) -> ClockParams {
        deserr::deserialize::<ClockParams, _, crate::param_errors::ModuleParamErrors>(json).unwrap()
    }

    #[test]
    fn clock_time_sig_deserialization() {
        // Verify time sig params deserialize correctly from JSON
        let params = deserialize_clock_params(
            serde_json::json!({"tempo": 120, "numerator": 6, "denominator": 8}),
        );
        assert_eq!(params.numerator, 6);
        assert_eq!(params.denominator, 8);
    }

    #[test]
    fn clock_tempo_deserialization() {
        let params = deserialize_clock_params(
            serde_json::json!({"tempo": 140, "numerator": 4, "denominator": 4}),
        );
        assert!((params.tempo - 140.0).abs() < 1e-9);
    }

    #[test]
    fn clock_required_params_rejected_when_missing() {
        // All params (tempo, numerator, denominator) are required
        let result = deserr::deserialize::<ClockParams, _, crate::param_errors::ModuleParamErrors>(
            serde_json::json!({}),
        );
        assert!(
            result.is_err(),
            "Empty JSON should fail: all clock params are required"
        );
    }

    #[test]
    fn clock_beat_trigger_resets_on_start() {
        let mut c = Clock::default();
        let sr = 48_000.0;

        // Advance partway through a bar
        for _ in 0..24_000 {
            c.update(sr);
        }

        // Start should reset beat phase
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

    #[test]
    fn clock_external_sync_overrides_phase() {
        let mut c = Clock::default();
        let sr = 48_000.0;

        // Advance clock to some non-zero phase
        for _ in 0..12_000 {
            c.update(sr);
        }
        let free_phase = c.state.phase;
        assert!(free_phase > 0.0);

        // Now sync to a specific phase externally
        c.sync_external_clock(0.75, 140.0);
        c.update(sr);

        // Phase should be near 0.75 (the externally-set value), not free-running
        assert!(
            (c.state.phase - 0.75).abs() < 0.01,
            "Phase should be near 0.75 after external sync, got {}",
            c.state.phase
        );
    }

    #[test]
    fn clock_external_sync_generates_beat_triggers() {
        let mut c = Clock::default();
        let sr = 48_000.0;

        // Simulate Link driving the clock through one full bar
        // 120 BPM, 4/4 time = 96000 samples per bar
        let samples_per_bar = 96_000;
        let mut beat_triggers = 0;
        let mut was_high = false;

        for i in 0..samples_per_bar {
            let bar_phase = i as f64 / samples_per_bar as f64;
            c.sync_external_clock(bar_phase, 120.0);
            c.update(sr);

            let is_high = c.outputs.beat_trigger == 5.0;
            if is_high && !was_high {
                beat_triggers += 1;
            }
            was_high = is_high;
        }

        assert_eq!(
            beat_triggers, 4,
            "External sync should generate 4 beat triggers per bar in 4/4, got {}",
            beat_triggers
        );
    }

    #[test]
    fn clock_external_sync_clears_on_none() {
        let mut c = Clock::default();
        let sr = 48_000.0;

        // Sync externally
        c.sync_external_clock(0.5, 120.0);
        c.update(sr);

        // Clear external sync
        c.clear_external_sync();
        let phase_before = c.state.phase;
        c.update(sr);

        // Should advance freely from where it was
        assert!(
            c.state.phase > phase_before,
            "Clock should free-run after clearing external sync"
        );
    }
}
