//! Reusable fixed-capacity delay line for audio DSP.
//!
//! Buffer is power-of-2 sized for efficient wrapping via bitwise AND.
//! All memory is allocated at construction time — no heap allocation
//! during audio processing.

/// A fixed-capacity delay line backed by a `Vec<f32>`.
///
/// The buffer is sized to the next power of 2 >= `max_delay + 1`,
/// enabling wrapping via bitwise AND instead of modulo.
pub struct DelayLine {
    buffer: Vec<f32>,
    write_ptr: usize,
    mask: usize,
}

impl Default for DelayLine {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            write_ptr: 0,
            mask: 0,
        }
    }
}

impl DelayLine {
    /// Create a new delay line that can hold up to `max_delay` samples.
    ///
    /// The internal buffer is rounded up to the next power of 2 for
    /// efficient wrapping. Panics if `max_delay` is 0.
    pub fn new(max_delay: usize) -> Self {
        assert!(max_delay > 0, "max_delay must be > 0");
        let size = (max_delay + 1).next_power_of_two();
        Self {
            buffer: vec![0.0; size],
            write_ptr: 0,
            mask: size - 1,
        }
    }

    /// Write a sample at the current write position and advance the pointer.
    #[inline]
    pub fn write(&mut self, sample: f32) {
        self.buffer[self.write_ptr & self.mask] = sample;
        self.write_ptr = self.write_ptr.wrapping_add(1);
    }

    /// Read a sample at `delay` samples behind the current write position.
    ///
    /// A delay of 0 reads the most recently written sample.
    #[inline]
    pub fn read(&self, delay: usize) -> f32 {
        self.buffer[self.write_ptr.wrapping_sub(1).wrapping_sub(delay) & self.mask]
    }

    /// Read with linear interpolation at a fractional delay.
    #[inline]
    pub fn read_linear(&self, delay: f32) -> f32 {
        let delay_int = delay as usize;
        let frac = delay - delay_int as f32;
        let a = self.read(delay_int);
        let b = self.read(delay_int + 1);
        a + frac * (b - a)
    }

    /// Process one sample through an allpass filter embedded in this delay line.
    ///
    /// Writes the input (mixed with feedback), reads from the delay tap,
    /// and returns the allpass output.
    #[inline]
    pub fn allpass(&mut self, input: f32, delay: usize, coefficient: f32) -> f32 {
        let delayed = self.read(delay);
        let write_val = input + coefficient * delayed;
        self.write(write_val);
        delayed - coefficient * write_val
    }

    /// Process one sample through an allpass filter with fractional-sample delay.
    ///
    /// Same as [`allpass`] but uses linear interpolation for the read tap,
    /// enabling smooth modulation of the delay length.
    #[inline]
    pub fn allpass_linear(&mut self, input: f32, delay: f32, coefficient: f32) -> f32 {
        let delayed = self.read_linear(delay);
        let write_val = input + coefficient * delayed;
        self.write(write_val);
        delayed - coefficient * write_val
    }

    /// Clear all samples to zero without deallocating.
    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write_ptr = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_power_of_two_buffer() {
        let dl = DelayLine::new(100);
        assert_eq!(dl.buffer.len(), 128); // next power of 2 >= 101
        assert_eq!(dl.mask, 127);
    }

    #[test]
    fn new_exact_power_of_two() {
        let dl = DelayLine::new(127);
        assert_eq!(dl.buffer.len(), 128); // 127+1 = 128, already power of 2
        assert_eq!(dl.mask, 127);
    }

    #[test]
    #[should_panic(expected = "max_delay must be > 0")]
    fn new_zero_panics() {
        DelayLine::new(0);
    }

    #[test]
    fn write_and_read_delay_zero() {
        let mut dl = DelayLine::new(10);
        dl.write(42.0);
        assert_eq!(dl.read(0), 42.0);
    }

    #[test]
    fn write_and_read_various_delays() {
        let mut dl = DelayLine::new(10);
        for i in 0..5 {
            dl.write(i as f32);
        }
        // Most recent write was 4.0
        assert_eq!(dl.read(0), 4.0);
        assert_eq!(dl.read(1), 3.0);
        assert_eq!(dl.read(2), 2.0);
        assert_eq!(dl.read(3), 1.0);
        assert_eq!(dl.read(4), 0.0);
    }

    #[test]
    fn wrapping_works() {
        let mut dl = DelayLine::new(4); // buffer size = 8
        // Write more samples than buffer size
        for i in 0..20 {
            dl.write(i as f32);
        }
        assert_eq!(dl.read(0), 19.0);
        assert_eq!(dl.read(1), 18.0);
    }

    #[test]
    fn read_linear_interpolation() {
        let mut dl = DelayLine::new(10);
        dl.write(0.0);
        dl.write(10.0);
        // delay 0.0 = most recent (10.0)
        assert_eq!(dl.read_linear(0.0), 10.0);
        // delay 1.0 = previous (0.0)
        assert_eq!(dl.read_linear(1.0), 0.0);
        // delay 0.5 = halfway between 10.0 and 0.0
        assert_eq!(dl.read_linear(0.5), 5.0);
        // delay 0.25 = 75% of 10.0 + 25% of 0.0
        assert_eq!(dl.read_linear(0.25), 7.5);
    }

    #[test]
    fn allpass_unity_gain() {
        // Allpass should have unity magnitude response
        let mut dl = DelayLine::new(100);
        let coeff = 0.5;
        let delay = 10;

        // Feed an impulse and collect output energy
        let mut input_energy = 0.0f64;
        let mut output_energy = 0.0f64;

        // Impulse
        let out = dl.allpass(1.0, delay, coeff);
        input_energy += 1.0;
        output_energy += (out as f64) * (out as f64);

        // Collect tail
        for _ in 0..200 {
            let out = dl.allpass(0.0, delay, coeff);
            output_energy += (out as f64) * (out as f64);
        }

        // Energy should be preserved (within floating point tolerance)
        assert!(
            (output_energy - input_energy).abs() < 0.01,
            "allpass energy: input={input_energy}, output={output_energy}"
        );
    }

    #[test]
    fn clear_zeros_buffer() {
        let mut dl = DelayLine::new(10);
        for i in 0..10 {
            dl.write(i as f32 + 1.0);
        }
        dl.clear();
        for i in 0..10 {
            assert_eq!(dl.read(i), 0.0);
        }
    }

    #[test]
    fn default_is_empty() {
        let dl = DelayLine::default();
        assert!(dl.buffer.is_empty());
        assert_eq!(dl.mask, 0);
    }

    #[test]
    fn allpass_linear_unity_gain() {
        // allpass_linear with fractional delay uses linear interpolation in the
        // feedback path, which introduces a mild low-pass effect and prevents
        // strict energy preservation. We verify output energy stays within a
        // reasonable bound (no blow-up or excessive loss).
        let mut dl = DelayLine::new(100);
        let coeff = 0.5;
        let delay = 10.5_f32; // fractional delay

        let mut input_energy = 0.0f64;
        let mut output_energy = 0.0f64;

        // Impulse
        let out = dl.allpass_linear(1.0, delay, coeff);
        input_energy += 1.0;
        output_energy += (out as f64) * (out as f64);

        // Collect tail
        for _ in 0..500 {
            let out = dl.allpass_linear(0.0, delay, coeff);
            output_energy += (out as f64) * (out as f64);
        }

        // Linear interpolation in the allpass feedback path makes it not
        // strictly allpass — some energy loss is expected.  We check that
        // output energy is bounded: no blow-up (< 1.1) and no catastrophic
        // loss (> 0.5).
        assert!(
            output_energy < 1.1 && output_energy > 0.5,
            "allpass_linear energy out of range: input={input_energy}, output={output_energy}"
        );
    }
}
