pub mod delay_line;
pub mod halfband;
pub mod one_pole;

use crate::dsp::consts::{LUT_PITCH_RATIO_HIGH, LUT_PITCH_RATIO_LOW};

// ============ Gate/Trigger Voltage Standards ============
// Gates and triggers output 5V when high, 0V when low.
// Detection uses a Schmitt trigger with hysteresis:
// - High threshold: 1.0V (signal goes high when input rises above this)
// - Low threshold: 0.1V (signal goes low when input falls below this)
pub const GATE_HIGH_VOLTAGE: f32 = 5.0;
pub const GATE_LOW_VOLTAGE: f32 = 0.0;
pub const GATE_DETECTION_HIGH_THRESHOLD: f32 = 1.0;
pub const GATE_DETECTION_LOW_THRESHOLD: f32 = 0.1;

/// Minimum duration for trigger pulses and gate retrigger gaps, in seconds.
/// Controls how long triggers stay high and how long gate retrigger gaps last.
/// At 48 kHz sample rate this equals approximately 16 samples.
pub const MIN_GATE_DURATION_SECS: f32 = 1.0 / 3000.0; // ~0.333 ms

/// Convert [`MIN_GATE_DURATION_SECS`] to a sample count for the given sample rate.
pub fn min_gate_samples(sample_rate: f32) -> u32 {
    (sample_rate * MIN_GATE_DURATION_SECS).round() as u32
}

#[inline]
pub fn changed(a: f32, b: f32) -> bool {
    // NaN is used as a sentinel for "never computed" in filter state,
    // so any NaN argument must be treated as a change.
    a.is_nan() || b.is_nan() || (a - b).abs() > 1e-6
}

/// Replace non-finite values (NaN, ±Inf) with 0.0 to prevent
/// sticky corruption of recursive filter state.
#[inline]
pub fn sanitize(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Map a value from one range to another. If the input range is degenerate, returns `y0`.
pub fn map_range(x: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    let denom = x1 - x0;
    if denom.abs() < f32::EPSILON {
        return y0;
    }
    (x - x0) * (y1 - y0) / denom + y0
}

fn make_integral_fractional(x: f32) -> (i32, f32) {
    let integral: i32 = x as i32;
    let fractional: f32 = x - (integral as f32);
    (integral, fractional)
}

pub fn semitones_to_ratio(semitones: f32) -> f32 {
    let pitch: f32 = semitones + 128.0;
    let (pitch_integral, pitch_fractional) = make_integral_fractional(pitch);

    LUT_PITCH_RATIO_HIGH[pitch_integral as usize]
        * LUT_PITCH_RATIO_LOW[(pitch_fractional * 256.0) as usize]
}

pub fn interpolate(table: &'static [f32], mut index: f32, size: usize) -> f32 {
    index *= size as f32;
    let (index_integral, index_fractional) = make_integral_fractional(index);
    let a: f32 = table[index_integral as usize];
    let b: f32 = table[(index_integral + 1) as usize];
    a + (b - a) * index_fractional
}

pub fn wrap<T>(range: std::ops::Range<T>, val: T) -> T
where
    T: std::ops::Sub<Output = T>
        + std::ops::Add<Output = T>
        + std::ops::Rem<Output = T>
        + std::cmp::PartialOrd
        + Copy,
{
    let span = range.end - range.start;
    let offset = (val - range.start) % span;
    let zero = span - span;
    let offset = if offset < zero { offset + span } else { offset };
    range.start + offset
}

/// Result of edge detection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeEvent {
    /// No state change
    #[default]
    None,
    /// Transitioned from low to high
    Rising,
    /// Transitioned from high to low
    Falling,
}

impl EdgeEvent {
    /// Returns true if this is a rising edge
    pub fn is_rising(&self) -> bool {
        matches!(self, EdgeEvent::Rising)
    }

    /// Returns true if this is a falling edge
    pub fn is_falling(&self) -> bool {
        matches!(self, EdgeEvent::Falling)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum SchmittState {
    Low,
    High,
    #[default]
    Uninitialized,
}

/// Reusable Schmitt trigger with hysteresis
#[derive(Debug, Clone, Copy)]
pub struct SchmittTrigger {
    pub state: SchmittState,
    low_threshold: f32,
    high_threshold: f32,
}

impl SchmittTrigger {
    /// Create a new Schmitt trigger with the given thresholds
    pub fn new(low_threshold: f32, high_threshold: f32) -> Self {
        Self {
            state: SchmittState::Uninitialized,
            low_threshold,
            high_threshold,
        }
    }

    /// Process a sample through the Schmitt trigger
    /// Returns true if it toggled from low to high (rising edge)
    pub fn process(&mut self, input: f32) -> bool {
        self.process_with_edge(input).1.is_rising()
    }

    /// Process a sample through the Schmitt trigger
    /// Returns (is_high, edge_event) where is_high is the current state and edge_event indicates any transition
    pub fn process_with_edge(&mut self, input: f32) -> (bool, EdgeEvent) {
        match self.state {
            SchmittState::Uninitialized => {
                // Initialize state based on input, reporting edges for definitive states
                if input >= self.high_threshold {
                    self.state = SchmittState::High;
                    (true, EdgeEvent::Rising)
                } else if input < self.low_threshold {
                    self.state = SchmittState::Low;
                    (false, EdgeEvent::Falling)
                } else {
                    // In hysteresis band — no definitive edge
                    self.state = SchmittState::Low;
                    (false, EdgeEvent::None)
                }
            }
            SchmittState::High => {
                // Currently high - check if we should go low
                if input < self.low_threshold {
                    self.state = SchmittState::Low;
                    (false, EdgeEvent::Falling)
                } else {
                    (true, EdgeEvent::None)
                }
            }
            SchmittState::Low => {
                // Currently low - check if we should go high
                if input >= self.high_threshold {
                    self.state = SchmittState::High;
                    (true, EdgeEvent::Rising)
                } else {
                    (false, EdgeEvent::None)
                }
            }
        }
    }

    /// Update thresholds
    pub fn set_thresholds(&mut self, low_threshold: f32, high_threshold: f32) {
        self.low_threshold = low_threshold;
        self.high_threshold = high_threshold;
    }

    /// Get current state
    pub fn state(&self) -> SchmittState {
        self.state
    }

    /// Reset state to Uninitialized
    pub fn reset(&mut self) {
        self.state = SchmittState::Uninitialized;
    }
}

impl Default for SchmittTrigger {
    fn default() -> Self {
        Self::new(GATE_DETECTION_LOW_THRESHOLD, GATE_DETECTION_HIGH_THRESHOLD)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TempGateState {
    #[default]
    Low,
    High,
}

/// Temporary gate/trigger generator with configurable hold duration.
///
/// Outputs a voltage corresponding to its current state (`low_val` or
/// `high_val`) and holds that state for a configurable number of samples
/// before transitioning to a target state.
///
/// # Typical usage
///
/// | Intent             | Call                         | Effect                            |
/// |--------------------|------------------------------|-----------------------------------|
/// | Trigger pulse      | `set_state(High, Low, hold)` | High for `hold` samples, then Low |
/// | Gate retrigger gap | `set_state(Low, High, hold)` | Low for `hold` samples, then High |
/// | Gate off           | `set_state(Low, Low, 0)`     | Immediately Low                   |
///
/// The `hold_samples` parameter controls how many calls to [`process`](TempGate::process)
/// will output the initial `state` before transitioning to `target`.
/// Use [`min_gate_samples`] to derive this from the current sample rate.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TempGate {
    target: TempGateState,
    state: TempGateState,
    low_val: f32,
    high_val: f32,
    /// Samples remaining before transitioning from `state` to `target`.
    counter: u32,
}

impl Default for TempGate {
    fn default() -> Self {
        Self::new_gate(TempGateState::Low)
    }
}

impl TempGate {
    pub fn new(state: TempGateState, low_val: f32, high_val: f32) -> Self {
        Self {
            target: state,
            state,
            low_val,
            high_val,
            counter: 0,
        }
    }

    /// Create a TempGate using standard gate voltages (0V low, 5V high)
    pub fn new_gate(state: TempGateState) -> Self {
        Self::new(state, GATE_LOW_VOLTAGE, GATE_HIGH_VOLTAGE)
    }

    /// Set the current state and the target state to transition to after
    /// `hold_samples` calls to [`process`](TempGate::process).
    ///
    /// When `state == target` (e.g. gate-off), pass `hold_samples = 0`.
    pub fn set_state(&mut self, state: TempGateState, target: TempGateState, hold_samples: u32) {
        self.state = state;
        self.target = target;
        self.counter = hold_samples;
    }

    /// Output the current state voltage and advance the hold counter.
    ///
    /// Returns `high_val` while in [`TempGateState::High`] and `low_val`
    /// while in [`TempGateState::Low`]. After `hold_samples` calls the
    /// state transitions to `target`.
    pub fn process(&mut self) -> f32 {
        let output = match self.state {
            TempGateState::Low => self.low_val,
            TempGateState::High => self.high_val,
        };
        if self.state != self.target {
            self.counter = self.counter.saturating_sub(1);
            if self.counter == 0 {
                self.state = self.target;
            }
        }
        output
    }
}

// ============ Pitch Conversion Functions ============
// Convention: 0V = C4 = MIDI 60 = ~261.626 Hz
// C4 = A4 / 2^(9/12) where A4 = 440 Hz
pub const C4_HZ_F64: f64 = 261.6255653005986; // 440.0 / 2^(9/12)
pub const C4_HZ_F32: f32 = 261.625_58; // 440.0 / 2^(9/12)

pub fn hz_to_voct_f64(frequency_hz: f64) -> f64 {
    (frequency_hz / C4_HZ_F64).log2()
}

/// Convert MIDI note number to V/Oct (C4 = 0V = MIDI 60)
pub fn midi_to_voct_f64(midi: f64) -> f64 {
    (midi - 60.0) / 12.0
}

/// Convert V/Oct to MIDI note number
pub fn voct_to_midi_f64(voct: f64) -> f64 {
    voct * 12.0 + 60.0
}

/// Convert V/Oct to frequency in Hz
pub fn voct_to_hz_f64(voct: f64) -> f64 {
    C4_HZ_F64 * 2.0_f64.powf(voct)
}

pub fn hz_to_voct(frequency_hz: f32) -> f32 {
    (frequency_hz / C4_HZ_F32).log2()
}

/// Convert MIDI note number to V/Oct (C4 = 0V = MIDI 60)
pub fn midi_to_voct(midi: f32) -> f32 {
    (midi - 60.0) / 12.0
}

/// Convert V/Oct to MIDI note number
pub fn voct_to_midi(voct: f32) -> f32 {
    voct * 12.0 + 60.0
}

/// Convert V/Oct to frequency in Hz
pub fn voct_to_hz(voct: f32) -> f32 {
    C4_HZ_F32 * 2.0_f32.powf(voct)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for make_integral_fractional
    #[test]
    fn test_make_integral_fractional_positive() {
        let (integral, fractional) = make_integral_fractional(3.75);
        assert_eq!(integral, 3);
        assert!((fractional - 0.75).abs() < 0.0001);
    }

    #[test]
    fn test_make_integral_fractional_whole_number() {
        let (integral, fractional) = make_integral_fractional(5.0);
        assert_eq!(integral, 5);
        assert!((fractional - 0.0).abs() < 0.0001);
    }

    #[test]
    fn test_make_integral_fractional_negative() {
        let (integral, fractional) = make_integral_fractional(-2.25);
        assert_eq!(integral, -2);
        // -2.25 - (-2) = -0.25
        assert!((fractional - (-0.25)).abs() < 0.0001);
    }

    // Tests for semitones_to_ratio
    #[test]
    fn test_semitones_to_ratio_zero() {
        // 0 semitones = ratio of 1.0
        let ratio = semitones_to_ratio(0.0);
        assert!(
            (ratio - 1.0).abs() < 0.01,
            "0 semitones should be ratio 1.0, got {}",
            ratio
        );
    }

    #[test]
    fn test_semitones_to_ratio_octave_up() {
        // 12 semitones = ratio of 2.0 (one octave up)
        let ratio = semitones_to_ratio(12.0);
        assert!(
            (ratio - 2.0).abs() < 0.01,
            "12 semitones should be ratio 2.0, got {}",
            ratio
        );
    }

    #[test]
    fn test_semitones_to_ratio_octave_down() {
        // -12 semitones = ratio of 0.5 (one octave down)
        let ratio = semitones_to_ratio(-12.0);
        assert!(
            (ratio - 0.5).abs() < 0.01,
            "-12 semitones should be ratio 0.5, got {}",
            ratio
        );
    }

    #[test]
    fn test_semitones_to_ratio_perfect_fifth() {
        // 7 semitones = ratio of ~1.498 (perfect fifth)
        let ratio = semitones_to_ratio(7.0);
        assert!(
            (ratio - 1.498).abs() < 0.01,
            "7 semitones should be ~1.498, got {}",
            ratio
        );
    }

    // Tests for wrap
    #[test]
    fn test_wrap_within_range() {
        let result: f32 = wrap(0.0..1.0, 0.5);
        assert!((result - 0.5).abs() < 0.0001);
    }

    #[test]
    fn test_wrap_above_range() {
        let result: f32 = wrap(0.0..1.0, 1.5);
        assert!(
            (result - 0.5).abs() < 0.0001,
            "1.5 wrapped to 0-1 should be 0.5, got {}",
            result
        );
    }

    #[test]
    fn test_wrap_below_range() {
        let result: f32 = wrap(0.0..1.0, -0.25);
        assert!(
            (result - 0.75).abs() < 0.0001,
            "-0.25 wrapped to 0-1 should be 0.75, got {}",
            result
        );
    }

    #[test]
    fn test_wrap_multiple_times_above() {
        let result: f32 = wrap(0.0..1.0, 2.75);
        assert!(
            (result - 0.75).abs() < 0.0001,
            "2.75 wrapped to 0-1 should be 0.75, got {}",
            result
        );
    }

    #[test]
    fn test_wrap_phase_range() {
        // Common use case: wrapping phase in range 0..2*PI
        use std::f32::consts::PI;
        let result: f32 = wrap(0.0..(2.0 * PI), 3.0 * PI);
        assert!(
            (result - PI).abs() < 0.0001,
            "3*PI wrapped to 0..2*PI should be PI, got {}",
            result
        );
    }

    #[test]
    fn test_wrap_integer() {
        let result = wrap(0..10, 15);
        assert_eq!(result, 5);
    }

    #[test]
    fn test_map_range() {
        assert!((map_range(0.5, 0.0, 1.0, -1.0, 1.0) - 0.0).abs() < 1e-6);
        assert_eq!(map_range(1.0, 1.0, 1.0, 2.0, 4.0), 2.0);
    }

    // Exhaustive-ish tests for pitch conversion functions
    #[test]
    fn test_pitch_roundtrip_midi_voct_f64() {
        for midi in 0..=127 {
            let midi_f = midi as f64;
            let voct = midi_to_voct_f64(midi_f);
            let roundtrip = voct_to_midi_f64(voct);
            assert!(
                (roundtrip - midi_f).abs() < 1e-9,
                "midi->voct->midi should roundtrip, got {} for {}",
                roundtrip,
                midi_f
            );
        }
    }

    #[test]
    fn test_pitch_roundtrip_midi_voct_f32() {
        for midi in 0..=127 {
            let midi_f = midi as f32;
            let voct = midi_to_voct(midi_f);
            let roundtrip = voct_to_midi(voct);
            assert!(
                (roundtrip - midi_f).abs() < 1e-5,
                "midi->voct->midi should roundtrip, got {} for {}",
                roundtrip,
                midi_f
            );
        }
    }

    #[test]
    fn test_pitch_roundtrip_voct_hz_f64() {
        for steps in -120..=120 {
            let voct = steps as f64 / 12.0;
            let hz = voct_to_hz_f64(voct);
            let roundtrip = hz_to_voct_f64(hz);
            assert!(
                (roundtrip - voct).abs() < 1e-9,
                "voct->hz->voct should roundtrip, got {} for {}",
                roundtrip,
                voct
            );
        }
    }

    #[test]
    fn test_pitch_roundtrip_voct_hz_f32() {
        for steps in -120..=120 {
            let voct = steps as f32 / 12.0;
            let hz = voct_to_hz(voct);
            let roundtrip = hz_to_voct(hz);
            assert!(
                (roundtrip - voct).abs() < 1e-5,
                "voct->hz->voct should roundtrip, got {} for {}",
                roundtrip,
                voct
            );
        }
    }

    #[test]
    fn test_pitch_expected_anchors_f64() {
        // C4 = MIDI 60 = 0V = C4_HZ_F64 Hz
        let voct_c4 = midi_to_voct_f64(60.0);
        assert!((voct_c4 - 0.0).abs() < 1e-12);
        let hz_c4 = voct_to_hz_f64(0.0);
        assert!((hz_c4 - C4_HZ_F64).abs() < 1e-12);

        // A4 = MIDI 69 = 0.75V = 440 Hz (9 semitones above C4)
        let voct_a4 = midi_to_voct_f64(69.0);
        assert!((voct_a4 - 0.75).abs() < 1e-12);
        let hz_a4 = voct_to_hz_f64(0.75);
        assert!((hz_a4 - 440.0).abs() < 1e-9);
    }

    #[test]
    fn test_pitch_expected_anchors_f32() {
        // C4 = MIDI 60 = 0V = C4_HZ_F32 Hz
        let voct_c4 = midi_to_voct(60.0);
        assert!((voct_c4 - 0.0).abs() < 1e-6);
        let hz_c4 = voct_to_hz(0.0);
        assert!((hz_c4 - C4_HZ_F32).abs() < 1e-4);

        // A4 = MIDI 69 = 0.75V = 440 Hz (9 semitones above C4)
        let voct_a4 = midi_to_voct(69.0);
        assert!((voct_a4 - 0.75).abs() < 1e-6);
        let hz_a4 = voct_to_hz(0.75);
        assert!((hz_a4 - 440.0).abs() < 1e-4);
    }

    #[test]
    fn test_pitch_arbitrary_cents_f64() {
        // A4 = 440Hz = 0.75V, plus/minus cents
        let cases = [
            (25.0, 0.75 + 25.0 / 1200.0),
            (-37.0, 0.75 - 37.0 / 1200.0),
            (123.0, 0.75 + 123.0 / 1200.0),
        ];
        for (cents, expected_voct) in cases {
            let freq = 440.0 * 2.0_f64.powf(cents / 1200.0);
            let voct = hz_to_voct_f64(freq);
            assert!(
                (voct - expected_voct).abs() < 1e-9,
                "hz_to_voct_f64 cents {} got {} expected {}",
                cents,
                voct,
                expected_voct
            );

            let freq_roundtrip = voct_to_hz_f64(expected_voct);
            assert!(
                (freq_roundtrip - freq).abs() < 1e-9,
                "voct_to_hz_f64 cents {} got {} expected {}",
                cents,
                freq_roundtrip,
                freq
            );
        }
    }

    #[test]
    fn test_pitch_arbitrary_cents_f32() {
        // A4 = 440Hz = 0.75V, plus/minus cents
        let cases = [
            (25.0_f32, 0.75_f32 + 25.0_f32 / 1200.0_f32),
            (-37.0_f32, 0.75_f32 - 37.0_f32 / 1200.0_f32),
            (123.0_f32, 0.75_f32 + 123.0_f32 / 1200.0_f32),
        ];
        for (cents, expected_voct) in cases {
            let freq = 440.0_f32 * 2.0_f32.powf(cents / 1200.0_f32);
            let voct = hz_to_voct(freq);
            assert!(
                (voct - expected_voct).abs() < 1e-5,
                "hz_to_voct cents {} got {} expected {}",
                cents,
                voct,
                expected_voct
            );

            let freq_roundtrip = voct_to_hz(expected_voct);
            assert!(
                (freq_roundtrip - freq).abs() < 1e-4,
                "voct_to_hz cents {} got {} expected {}",
                cents,
                freq_roundtrip,
                freq
            );
        }
    }

    #[test]
    fn test_pitch_arbitrary_fractional_midi_f64() {
        // C4 = MIDI 60 = 0V
        let cases = [
            (69.5, 0.75 + 0.5 / 12.0), // A4 + 50 cents
            (60.25, 0.25 / 12.0),      // C4 + 25 cents
            (72.0, 1.0),               // C5 = 1V
        ];
        for (midi, expected_voct) in cases {
            let voct = midi_to_voct_f64(midi);
            assert!((voct - expected_voct).abs() < 1e-12);

            let midi_roundtrip = voct_to_midi_f64(expected_voct);
            assert!((midi_roundtrip - midi).abs() < 1e-12);
        }
    }

    #[test]
    fn test_pitch_arbitrary_fractional_midi_f32() {
        // C4 = MIDI 60 = 0V
        let cases = [
            (69.5_f32, 0.75_f32 + 0.5_f32 / 12.0_f32), // A4 + 50 cents
            (60.25_f32, 0.25_f32 / 12.0_f32),          // C4 + 25 cents
            (72.0_f32, 1.0_f32),                       // C5 = 1V
        ];
        for (midi, expected_voct) in cases {
            let voct = midi_to_voct(midi);
            assert!((voct - expected_voct).abs() < 1e-6);

            let midi_roundtrip = voct_to_midi(expected_voct);
            assert!((midi_roundtrip - midi).abs() < 1e-5);
        }
    }

    // ============ SchmittTrigger Tests ============

    #[test]
    fn schmitt_default_thresholds() {
        let st = SchmittTrigger::default();
        assert_eq!(st.state(), SchmittState::Uninitialized);
        assert_eq!(st.low_threshold, GATE_DETECTION_LOW_THRESHOLD);
        assert_eq!(st.high_threshold, GATE_DETECTION_HIGH_THRESHOLD);
    }

    #[test]
    fn schmitt_custom_thresholds() {
        let st = SchmittTrigger::new(0.2, 0.8);
        assert_eq!(st.state(), SchmittState::Uninitialized);
        assert_eq!(st.low_threshold, 0.2);
        assert_eq!(st.high_threshold, 0.8);
    }

    #[test]
    fn schmitt_uninitialized_goes_high_at_threshold() {
        // >= high_threshold should transition to High
        let mut st = SchmittTrigger::new(0.1, 1.0);
        let (is_high, edge) = st.process_with_edge(1.0);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::Rising);
        assert_eq!(st.state(), SchmittState::High);
    }

    #[test]
    fn schmitt_uninitialized_goes_low_below_low_threshold() {
        let mut st = SchmittTrigger::new(0.1, 1.0);
        let (is_high, edge) = st.process_with_edge(0.05);
        assert!(!is_high);
        assert_eq!(edge, EdgeEvent::Falling);
        assert_eq!(st.state(), SchmittState::Low);
    }

    #[test]
    fn schmitt_uninitialized_in_hysteresis_band_no_edge() {
        // Value between low and high threshold — goes Low with no edge
        let mut st = SchmittTrigger::new(0.1, 1.0);
        let (is_high, edge) = st.process_with_edge(0.5);
        assert!(!is_high);
        assert_eq!(edge, EdgeEvent::None);
        assert_eq!(st.state(), SchmittState::Low);
    }

    #[test]
    fn schmitt_low_to_high_at_exact_threshold() {
        // Verify >= semantics: input exactly at high_threshold triggers
        let mut st = SchmittTrigger::new(0.1, 1.0);
        st.state = SchmittState::Low;
        let (is_high, edge) = st.process_with_edge(1.0);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::Rising);
    }

    #[test]
    fn schmitt_low_to_high_above_threshold() {
        let mut st = SchmittTrigger::new(0.1, 1.0);
        st.state = SchmittState::Low;
        let (is_high, edge) = st.process_with_edge(5.0);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::Rising);
    }

    #[test]
    fn schmitt_low_stays_low_just_below_threshold() {
        let mut st = SchmittTrigger::new(0.1, 1.0);
        st.state = SchmittState::Low;
        let (is_high, edge) = st.process_with_edge(0.999);
        assert!(!is_high);
        assert_eq!(edge, EdgeEvent::None);
    }

    #[test]
    fn schmitt_high_to_low_below_low_threshold() {
        let mut st = SchmittTrigger::new(0.1, 1.0);
        st.state = SchmittState::High;
        let (is_high, edge) = st.process_with_edge(0.05);
        assert!(!is_high);
        assert_eq!(edge, EdgeEvent::Falling);
    }

    #[test]
    fn schmitt_high_stays_high_at_low_threshold() {
        // < low_threshold is required to go low; exactly at it stays high
        let mut st = SchmittTrigger::new(0.1, 1.0);
        st.state = SchmittState::High;
        let (is_high, edge) = st.process_with_edge(0.1);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::None);
    }

    #[test]
    fn schmitt_high_stays_high_in_hysteresis_band() {
        let mut st = SchmittTrigger::new(0.1, 1.0);
        st.state = SchmittState::High;
        let (is_high, edge) = st.process_with_edge(0.5);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::None);
    }

    #[test]
    fn schmitt_full_cycle_low_high_low() {
        let mut st = SchmittTrigger::default(); // 0.1, 1.0
        // Start from uninitialized with low input
        let (_, e) = st.process_with_edge(0.0);
        assert_eq!(e, EdgeEvent::Falling);
        assert_eq!(st.state(), SchmittState::Low);

        // Stay low in hysteresis band
        let (_, e) = st.process_with_edge(0.5);
        assert_eq!(e, EdgeEvent::None);
        assert_eq!(st.state(), SchmittState::Low);

        // Rise above high threshold → Rising
        let (_, e) = st.process_with_edge(5.0);
        assert_eq!(e, EdgeEvent::Rising);
        assert_eq!(st.state(), SchmittState::High);

        // Stay high above low threshold
        let (_, e) = st.process_with_edge(0.5);
        assert_eq!(e, EdgeEvent::None);
        assert_eq!(st.state(), SchmittState::High);

        // Drop below low threshold → Falling
        let (_, e) = st.process_with_edge(0.0);
        assert_eq!(e, EdgeEvent::Falling);
        assert_eq!(st.state(), SchmittState::Low);
    }

    #[test]
    fn schmitt_process_returns_true_only_on_rising() {
        let mut st = SchmittTrigger::default();
        assert!(!st.process(0.0)); // Uninitialized → Low (Falling, not Rising)
        assert!(!st.process(0.5)); // Low → Low (None)
        assert!(st.process(5.0)); // Low → High (Rising)
        assert!(!st.process(5.0)); // High → High (None)
        assert!(!st.process(0.0)); // High → Low (Falling)
        assert!(st.process(5.0)); // Low → High (Rising)
    }

    #[test]
    fn schmitt_zero_thresholds_trigger_at_zero() {
        // This is the clock module pattern: SchmittTrigger::new(0.0, 0.0)
        let mut st = SchmittTrigger::new(0.0, 0.0);

        // Negative input: Uninitialized → Low (Falling)
        let (is_high, edge) = st.process_with_edge(-1.0);
        assert!(!is_high);
        assert_eq!(edge, EdgeEvent::Falling);

        // Still negative: stays Low
        let (is_high, edge) = st.process_with_edge(-0.5);
        assert!(!is_high);
        assert_eq!(edge, EdgeEvent::None);

        // Exactly 0.0: >= 0.0 is true → Rising
        let (is_high, edge) = st.process_with_edge(0.0);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::Rising);

        // Positive: stays High
        let (is_high, edge) = st.process_with_edge(1.0);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::None);

        // Exactly 0.0 again: < 0.0 is false, stays High
        let (is_high, edge) = st.process_with_edge(0.0);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::None);

        // Negative: < 0.0 → Falling
        let (is_high, edge) = st.process_with_edge(-0.001);
        assert!(!is_high);
        assert_eq!(edge, EdgeEvent::Falling);
    }

    #[test]
    fn schmitt_zero_thresholds_no_oscillation_at_zero() {
        // With (0.0, 0.0) thresholds, a sustained 0.0 input should not
        // oscillate: High→Low requires < 0.0, which 0.0 does not satisfy.
        let mut st = SchmittTrigger::new(0.0, 0.0);
        // Force to Low first
        st.process_with_edge(-1.0);
        assert_eq!(st.state(), SchmittState::Low);

        // First 0.0 → Rising
        let (_, edge) = st.process_with_edge(0.0);
        assert_eq!(edge, EdgeEvent::Rising);

        // Subsequent 0.0 → stays High, no edge
        for _ in 0..100 {
            let (is_high, edge) = st.process_with_edge(0.0);
            assert!(is_high);
            assert_eq!(edge, EdgeEvent::None);
        }
    }

    #[test]
    fn schmitt_reset_returns_to_uninitialized() {
        let mut st = SchmittTrigger::default();
        st.process(5.0); // Go to High
        assert_eq!(st.state(), SchmittState::High);

        st.reset();
        assert_eq!(st.state(), SchmittState::Uninitialized);

        // Next process from Uninitialized should report a rising edge
        let (is_high, edge) = st.process_with_edge(5.0);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::Rising);
    }

    #[test]
    fn schmitt_set_thresholds_updates_values() {
        let mut st = SchmittTrigger::default();
        st.process_with_edge(0.0); // Establish Low state
        assert_eq!(st.state(), SchmittState::Low);

        // Change to narrow thresholds
        st.set_thresholds(0.4, 0.6);

        // 0.5 is below new high threshold (0.6) → stays Low
        let (is_high, _) = st.process_with_edge(0.5);
        assert!(!is_high);

        // 0.6 is at new high threshold → Rising
        let (is_high, edge) = st.process_with_edge(0.6);
        assert!(is_high);
        assert_eq!(edge, EdgeEvent::Rising);
    }

    #[test]
    fn schmitt_equal_thresholds_used_by_scope() {
        // ScopeBuffer sets both thresholds equal via set_thresholds(thresh, thresh)
        let mut st = SchmittTrigger::new(0.5, 0.5);

        // Below threshold
        st.process_with_edge(-1.0);
        assert_eq!(st.state(), SchmittState::Low);

        // At threshold: >= 0.5 → Rising
        let (_, edge) = st.process_with_edge(0.5);
        assert_eq!(edge, EdgeEvent::Rising);

        // Below threshold: < 0.5 → Falling
        let (_, edge) = st.process_with_edge(0.49);
        assert_eq!(edge, EdgeEvent::Falling);
    }

    #[test]
    fn schmitt_rapid_transitions_only_report_edges() {
        let mut st = SchmittTrigger::default();
        st.process_with_edge(0.0); // Low

        let mut rising_count = 0;
        let mut falling_count = 0;

        // Simulate 10 rapid on/off cycles
        for _ in 0..10 {
            let (_, edge) = st.process_with_edge(5.0);
            if edge.is_rising() {
                rising_count += 1;
            }
            let (_, edge) = st.process_with_edge(0.0);
            if edge.is_falling() {
                falling_count += 1;
            }
        }

        assert_eq!(rising_count, 10);
        assert_eq!(falling_count, 10);
    }

    // ============ EdgeEvent Tests ============

    #[test]
    fn edge_event_is_rising() {
        assert!(EdgeEvent::Rising.is_rising());
        assert!(!EdgeEvent::Falling.is_rising());
        assert!(!EdgeEvent::None.is_rising());
    }

    #[test]
    fn edge_event_is_falling() {
        assert!(EdgeEvent::Falling.is_falling());
        assert!(!EdgeEvent::Rising.is_falling());
        assert!(!EdgeEvent::None.is_falling());
    }

    #[test]
    fn edge_event_default_is_none() {
        assert_eq!(EdgeEvent::default(), EdgeEvent::None);
    }

    // ============ TempGate Tests ============

    #[test]
    fn temp_gate_default_is_low_zero_volts() {
        let mut tg = TempGate::default();
        assert_eq!(tg.state, TempGateState::Low);
        assert_eq!(tg.process(), 0.0);
    }

    #[test]
    fn temp_gate_new_gate_uses_standard_voltages() {
        let mut tg = TempGate::new_gate(TempGateState::High);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);

        let mut tg = TempGate::new_gate(TempGateState::Low);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
    }

    #[test]
    fn temp_gate_new_sets_initial_state() {
        let tg = TempGate::new(TempGateState::High, 0.0, 5.0);
        assert_eq!(tg.state, TempGateState::High);
        assert_eq!(tg.target, TempGateState::High);
        assert_eq!(tg.counter, 0);
    }

    #[test]
    fn temp_gate_trigger_pulse_holds_then_transitions() {
        // Trigger pulse: High for N samples, then Low
        let mut tg = TempGate::new_gate(TempGateState::Low);
        tg.set_state(TempGateState::High, TempGateState::Low, 4);

        // Should output High for 4 samples
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);

        // 5th sample: transitioned to Low
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
        // Stays Low
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
    }

    #[test]
    fn temp_gate_retrigger_gap_holds_then_transitions() {
        // Gate retrigger gap: Low for N samples, then High
        let mut tg = TempGate::new_gate(TempGateState::High);
        tg.set_state(TempGateState::Low, TempGateState::High, 3);

        // Should output Low for 3 samples
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);

        // 4th sample: transitioned to High
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
    }

    #[test]
    fn temp_gate_gate_off_immediate() {
        // Gate off: set_state(Low, Low, 0) — immediate Low
        let mut tg = TempGate::new_gate(TempGateState::High);
        tg.set_state(TempGateState::Low, TempGateState::Low, 0);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
    }

    #[test]
    fn temp_gate_same_state_no_countdown() {
        // When state == target, counter should not matter
        let mut tg = TempGate::new_gate(TempGateState::Low);
        tg.set_state(TempGateState::High, TempGateState::High, 0);

        // Stays High indefinitely
        for _ in 0..100 {
            assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        }
    }

    #[test]
    fn temp_gate_hold_samples_1_produces_one_sample() {
        let mut tg = TempGate::new_gate(TempGateState::Low);
        tg.set_state(TempGateState::High, TempGateState::Low, 1);

        // Exactly 1 High sample
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        // Then Low
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
    }

    #[test]
    fn temp_gate_set_state_resets_counter() {
        // Trigger pulse mid-hold should restart
        let mut tg = TempGate::new_gate(TempGateState::Low);
        tg.set_state(TempGateState::High, TempGateState::Low, 4);

        // Consume 2 of 4 hold samples
        tg.process();
        tg.process();

        // Re-trigger with fresh hold
        tg.set_state(TempGateState::High, TempGateState::Low, 3);

        // Should get 3 more High samples
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
    }

    #[test]
    fn temp_gate_custom_voltages() {
        let mut tg = TempGate::new(TempGateState::Low, -1.0, 1.0);
        tg.set_state(TempGateState::High, TempGateState::Low, 2);

        assert_eq!(tg.process(), 1.0);
        assert_eq!(tg.process(), 1.0);
        assert_eq!(tg.process(), -1.0);
    }

    #[test]
    fn temp_gate_counter_saturates_at_zero() {
        // Even with state == target (no countdown happening), counter stays 0
        let mut tg = TempGate::new_gate(TempGateState::Low);
        for _ in 0..100 {
            tg.process();
        }
        assert_eq!(tg.counter, 0);
        assert_eq!(tg.state, TempGateState::Low);
    }

    #[test]
    fn temp_gate_trigger_then_retrigger_gap_sequence() {
        // Simulate: trigger pulse, settle, then retrigger gap
        let mut tg = TempGate::new_gate(TempGateState::Low);

        // Fire trigger pulse
        tg.set_state(TempGateState::High, TempGateState::Low, 2);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE); // settled

        // Now retrigger gap (gate was on, dip low briefly)
        tg.set_state(TempGateState::Low, TempGateState::High, 2);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
        assert_eq!(tg.process(), GATE_LOW_VOLTAGE);
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE); // back to High
        assert_eq!(tg.process(), GATE_HIGH_VOLTAGE); // stays
    }

    // ============ min_gate_samples Tests ============

    #[test]
    fn min_gate_samples_48khz() {
        let samples = min_gate_samples(48000.0);
        assert_eq!(samples, 16);
    }

    #[test]
    fn min_gate_samples_44100() {
        let samples = min_gate_samples(44100.0);
        // 44100 * (1/3000) = 14.7 → rounds to 15
        assert_eq!(samples, 15);
    }

    #[test]
    fn min_gate_samples_96khz() {
        let samples = min_gate_samples(96000.0);
        assert_eq!(samples, 32);
    }
}
