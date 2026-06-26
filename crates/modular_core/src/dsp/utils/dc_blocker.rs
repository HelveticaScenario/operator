//! One-pole high-pass (DC blocker) primitive.

use std::f32::consts::PI;

/// Corner frequency, in Hz, for de-rumbling a signal's DC and sub-audio
/// content. Low enough to leave the audible band untouched.
pub const DEFAULT_DC_BLOCK_FC_HZ: f32 = 20.0;

/// One-pole high-pass filter — `y[n] = x[n] - x[n-1] + R · y[n-1]`. Removes a
/// constant offset (and sub-audio rumble) while passing audio-rate content.
///
/// Holds the per-channel state (`prev_in`, `prev_out`). The feedback
/// coefficient `R` is sample-rate-only, so compute it once in `init` with
/// [`DcBlocker::coeff`] and pass it to [`DcBlocker::process`] each sample. This
/// mirrors the [`FadeIn`](super::fade::FadeIn) split: mutable per-channel state
/// here, the sample-rate constant computed statically.
#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlocker {
    /// Previous input sample (`x[n-1]`).
    pub prev_in: f32,
    /// Previous output sample (`y[n-1]`).
    pub prev_out: f32,
}

impl DcBlocker {
    /// Feedback coefficient `R` for a corner frequency at the given sample rate,
    /// clamped to `[0, 1]`. Sample-rate-only, so compute it once in `init`.
    #[inline]
    pub fn coeff(fc_hz: f32, sample_rate: f32) -> f32 {
        (1.0 - (2.0 * PI * fc_hz / sample_rate.max(1.0))).clamp(0.0, 1.0)
    }

    /// Process one sample through the high-pass, advancing the internal state.
    #[inline]
    pub fn process(&mut self, x: f32, coeff: f32) -> f32 {
        let y = x - self.prev_in + coeff * self.prev_out;
        self.prev_in = x;
        self.prev_out = y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_constant_offset() {
        let mut dc = DcBlocker::default();
        let coeff = DcBlocker::coeff(DEFAULT_DC_BLOCK_FC_HZ, 48000.0);
        let mut last = 0.0;
        for _ in 0..4000 {
            last = dc.process(1.0, coeff);
        }
        assert!(last.abs() < 0.05, "DC should decay toward 0, got {last}");
    }

    #[test]
    fn passes_audio_rate_signal() {
        let mut dc = DcBlocker::default();
        let coeff = DcBlocker::coeff(DEFAULT_DC_BLOCK_FC_HZ, 48000.0);
        // ~480 Hz, well above the 20 Hz corner — amplitude should survive.
        let freq_norm = 0.01_f32;
        let mut peak = 0.0_f32;
        for n in 0..4000 {
            let x = (2.0 * PI * freq_norm * n as f32).sin();
            let y = dc.process(x, coeff);
            if n >= 1000 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak > 0.9, "AC signal should pass, got peak {peak}");
    }

    #[test]
    fn process_matches_difference_equation() {
        // Lock the difference equation: y[n] = x[n] - x[n-1] + R·y[n-1].
        let coeff = 0.97_f32;
        let mut dc = DcBlocker::default();
        let xs = [0.5_f32, -0.2, 0.8, 0.8, 0.0];
        let mut prev_in = 0.0;
        let mut prev_out = 0.0;
        for &x in &xs {
            let expected = x - prev_in + coeff * prev_out;
            let got = dc.process(x, coeff);
            assert_eq!(got, expected);
            prev_in = x;
            prev_out = expected;
        }
    }

    #[test]
    fn coeff_clamped_to_unit_range() {
        // A tiny sample rate would push the raw coefficient negative; it clamps.
        assert_eq!(DcBlocker::coeff(20.0, 1.0), 0.0);
        // A high sample rate keeps it just below 1.
        let c = DcBlocker::coeff(DEFAULT_DC_BLOCK_FC_HZ, 48000.0);
        assert!(c > 0.99 && c < 1.0, "got {c}");
    }
}
