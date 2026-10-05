//! One-pole lowpass filter primitive.
//!
//! A simple first-order IIR filter: `y[n] = x[n] * coeff + y[n-1] * (1 - coeff)`.
//! Higher `coeff` values pass more high-frequency content.

use super::sanitize;

/// A one-pole (first-order) lowpass filter.
///
/// The coefficient controls the cutoff: values near 1.0 pass almost
/// everything, values near 0.0 heavily lowpass the signal.
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    coeff: f32,
    state: f32,
}

impl Default for OnePole {
    fn default() -> Self {
        Self {
            coeff: 0.5,
            state: 0.0,
        }
    }
}

impl OnePole {
    /// Create a new one-pole filter with the given coefficient (0..1).
    pub fn new(coeff: f32) -> Self {
        Self { coeff, state: 0.0 }
    }

    /// Update the filter coefficient.
    #[inline]
    pub fn set_coeff(&mut self, coeff: f32) {
        self.coeff = coeff;
    }

    /// Process one sample through the filter. The stored state is sanitized
    /// every sample so a non-finite input cannot lodge in the recursion — the
    /// filter recovers as soon as the input does.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        self.state = sanitize(input * self.coeff + self.state * (1.0 - self.coeff));
        self.state
    }

    /// Reset the filter state to zero.
    pub fn reset(&mut self) {
        self.state = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_half_coeff() {
        let f = OnePole::default();
        assert_eq!(f.coeff, 0.5);
        assert_eq!(f.state, 0.0);
    }

    #[test]
    fn new_sets_coeff() {
        let f = OnePole::new(0.7);
        assert_eq!(f.coeff, 0.7);
    }

    #[test]
    fn coeff_one_passes_through() {
        let mut f = OnePole::new(1.0);
        assert_eq!(f.process(1.0), 1.0);
        assert_eq!(f.process(0.5), 0.5);
        assert_eq!(f.process(0.0), 0.0);
    }

    #[test]
    fn coeff_zero_blocks_signal() {
        let mut f = OnePole::new(0.0);
        // With coeff=0, output is always previous state (starts at 0)
        assert_eq!(f.process(1.0), 0.0);
        assert_eq!(f.process(1.0), 0.0);
    }

    #[test]
    fn lowpass_behavior() {
        // A step input should ramp up gradually with coeff < 1
        let mut f = OnePole::new(0.1);
        let mut prev = 0.0;
        for _ in 0..100 {
            let out = f.process(1.0);
            assert!(out >= prev, "output should monotonically increase");
            prev = out;
        }
        // After many samples, should approach 1.0
        assert!(prev > 0.99, "should converge to input, got {prev}");
    }

    #[test]
    fn recovers_after_non_finite_input() {
        // A non-finite sample must not lodge in the recursion: once the input
        // is finite again, the filter converges to it as usual.
        let mut f = OnePole::new(0.1);
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            f.process(x);
        }
        let mut last = 0.0;
        for _ in 0..200 {
            last = f.process(1.0);
            assert!(last.is_finite());
        }
        assert!(
            last > 0.99,
            "should converge to input after recovery, got {last}"
        );
    }

    #[test]
    fn reset_clears_state() {
        let mut f = OnePole::new(0.5);
        f.process(1.0);
        assert!(f.state != 0.0);
        f.reset();
        assert_eq!(f.state, 0.0);
    }

    #[test]
    fn set_coeff_changes_behavior() {
        let mut f = OnePole::new(0.1);
        let slow = f.process(1.0);
        f.reset();
        f.set_coeff(0.9);
        let fast = f.process(1.0);
        assert!(fast > slow, "higher coeff should respond faster");
    }
}
