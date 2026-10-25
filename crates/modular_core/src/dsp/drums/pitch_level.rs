//! Levelling a pitched voice across its `freq` range. A resonator struck by
//! a fixed-width pulse rings louder the higher it is tuned, roughly doubling
//! per octave, so each pitched voice carries a measured correction that
//! holds an unaccented hit near the level it has at the stock pitch.

/// A voice's level correction: `octaves[k]` is the gain, in octaves, at
/// the `k`th of evenly spaced pitches from `lo` to `hi` V/Oct.
pub struct PitchLevel {
    pub lo: f32,
    pub hi: f32,
    pub octaves: &'static [f32],
}

impl PitchLevel {
    /// The output gain at `freq`, V/Oct, held at the range's ends beyond
    /// them.
    pub fn gain(&self, freq: f32) -> f32 {
        let n = self.octaves.len() - 1;
        let x = (freq - self.lo) / (self.hi - self.lo) * n as f32;
        // `max` before `min` maps NaN to the low end.
        let x = x.max(0.0).min(n as f32);
        let i = (x as usize).min(n - 1);
        let (a, b) = (self.octaves[i], self.octaves[i + 1]);
        (a + (b - a) * (x - i as f32)).exp2()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL: PitchLevel = PitchLevel {
        lo: 0.0,
        hi: 2.0,
        octaves: &[1.0, 0.0, -1.0],
    };

    #[test]
    fn interpolates_in_octaves_and_holds_beyond_the_range() {
        assert_eq!(LEVEL.gain(1.0), 1.0);
        assert!((LEVEL.gain(0.5) - 2f32.powf(0.5)).abs() < 1e-6);
        assert_eq!(LEVEL.gain(-3.0), 2.0);
        assert_eq!(LEVEL.gain(9.0), 0.5);
        assert_eq!(LEVEL.gain(f32::NAN), 2.0);
    }
}
