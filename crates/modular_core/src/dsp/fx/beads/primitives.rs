//! Small DSP building blocks shared by the Beads modules, ported from
//! Mutable Instruments' stmlib with the same numerics.

use std::ops::{Add, AddAssign, Mul, MulAssign, Sub};

/// One stereo sample, in hardware full-scale units (±1.0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub l: f32,
    pub r: f32,
}

impl Frame {
    pub const ZERO: Frame = Frame { l: 0.0, r: 0.0 };

    #[inline]
    pub fn new(l: f32, r: f32) -> Self {
        Self { l, r }
    }
}

impl Add for Frame {
    type Output = Frame;
    #[inline]
    fn add(self, rhs: Frame) -> Frame {
        Frame::new(self.l + rhs.l, self.r + rhs.r)
    }
}

impl AddAssign for Frame {
    #[inline]
    fn add_assign(&mut self, rhs: Frame) {
        self.l += rhs.l;
        self.r += rhs.r;
    }
}

impl Sub for Frame {
    type Output = Frame;
    #[inline]
    fn sub(self, rhs: Frame) -> Frame {
        Frame::new(self.l - rhs.l, self.r - rhs.r)
    }
}

impl Mul<f32> for Frame {
    type Output = Frame;
    #[inline]
    fn mul(self, rhs: f32) -> Frame {
        Frame::new(self.l * rhs, self.r * rhs)
    }
}

impl MulAssign<f32> for Frame {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.l *= rhs;
        self.r *= rhs;
    }
}

#[inline]
pub fn smooth_step(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x < -3.0 {
        -1.0
    } else if x > 3.0 {
        1.0
    } else {
        x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
    }
}

/// Saturate to the signed 16-bit range.
#[inline]
pub fn clip16(x: i32) -> i32 {
    x.clamp(-32768, 32767)
}

/// Asymmetric one-pole follower: `positive` applies while rising, `negative`
/// while falling.
#[inline]
pub fn slope(state: &mut f32, input: f32, positive: f32, negative: f32) {
    let error = input - *state;
    *state += if error > 0.0 { positive } else { negative } * error;
}

/// Linear ramp from the current value to a new target over one control block,
/// the per-block parameter smoothing used throughout the hardware.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ramp {
    value: f32,
    increment: f32,
}

impl Ramp {
    #[inline]
    pub fn start(&mut self, target: f32, block_size: usize) {
        self.increment = (target - self.value) / block_size as f32;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value += self.increment;
        self.value
    }

    #[inline]
    pub fn value(&self) -> f32 {
        self.value
    }

    #[inline]
    pub fn reset(&mut self, value: f32) {
        self.value = value;
        self.increment = 0.0;
    }
}

/// Peak limiter at full scale (±5 V) with instant attack, so no sample
/// leaves above it, and a two-stage release set by `decay` (a per-sample
/// coefficient).
#[derive(Clone, Copy, Debug, Default)]
pub struct Limiter {
    peak: f32,
    peak_lp: f32,
    decay: f32,
}

impl Limiter {
    pub fn init(&mut self, decay: f32) {
        self.peak = 0.5;
        self.peak_lp = 0.5;
        self.decay = decay;
    }

    /// Track the peak `level` of one sample and return the gain to apply.
    #[inline]
    pub fn gain(&mut self, level: f32) -> f32 {
        slope(&mut self.peak, level, 1.0, self.decay);
        let peak = self.peak;
        slope(&mut self.peak_lp, peak, 1.0, self.decay);
        if self.peak_lp <= 1.0 {
            1.0
        } else {
            1.0 / self.peak_lp
        }
    }

    #[inline]
    pub fn process(&mut self, frame: Frame) -> Frame {
        frame * self.gain(frame.l.abs().max(frame.r.abs()))
    }

    #[inline]
    pub fn process_mono(&mut self, x: f32) -> f32 {
        x * self.gain(x.abs())
    }
}

/// stmlib's fast approximation of `tan(π f)`, tuned for 16 Hz–16 kHz at
/// 48 kHz.
#[inline]
fn fast_tan(f: f32) -> f32 {
    use std::f32::consts::PI;
    let a = 3.260e-1 * PI * PI * PI;
    let b = 1.823e-1 * PI * PI * PI * PI * PI;
    let f2 = f * f;
    f * (PI + f2 * (a + b * f2))
}

/// Trapezoidal state-variable filter whose cutoff can move every sample
/// without resetting its state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    g: f32,
    r: f32,
    h: f32,
    state_1: f32,
    state_2: f32,
}

impl Svf {
    /// `f` is the cutoff as a fraction of the sample rate.
    #[inline]
    pub fn set_f_q(&mut self, f: f32, resonance: f32) {
        self.g = fast_tan(f);
        self.r = 1.0 / resonance;
        self.h = 1.0 / (1.0 + self.r * self.g + self.g * self.g);
    }

    /// Low-pass and high-pass outputs for `input`.
    #[inline]
    pub fn process(&mut self, input: f32) -> (f32, f32) {
        let hp = (input - self.r * self.state_1 - self.g * self.state_1 - self.state_2) * self.h;
        let bp = self.g * hp + self.state_1;
        self.state_1 = self.g * hp + bp;
        let lp = self.g * bp + self.state_2;
        self.state_2 = self.g * bp + lp;
        (lp, hp)
    }
}

/// stmlib's approximate cosine oscillator: a two-pole resonator whose output
/// stays in roughly `[0, 1]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CosineOscillator {
    y0: f32,
    y1: f32,
    iir_coefficient: f32,
}

impl CosineOscillator {
    /// `frequency` is in cycles per call to [`Self::next`].
    pub fn init_approximate(&mut self, frequency: f32) {
        let mut frequency = frequency - 0.25;
        let mut sign = 16.0;
        if frequency < 0.0 {
            frequency = -frequency;
        } else if frequency > 0.5 {
            frequency -= 0.5;
        } else {
            sign = -16.0;
        }
        self.iir_coefficient = sign * frequency * (1.0 - 2.0 * frequency);
        self.y1 = self.iir_coefficient * 0.25;
        self.y0 = 0.5;
    }

    #[inline]
    pub fn value(&self) -> f32 {
        self.y1 + 0.5
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let temp = self.y0;
        self.y0 = self.iir_coefficient * self.y0 - self.y1;
        self.y1 = temp;
        temp + 0.5
    }
}

const SINE_TABLE_SIZE: usize = 1024;

/// One sine cycle sampled at [`SINE_TABLE_SIZE`] points plus a guard point,
/// read with linear interpolation as the hardware does.
fn sine_table() -> &'static [f32; SINE_TABLE_SIZE + 1] {
    static TABLE: std::sync::OnceLock<[f32; SINE_TABLE_SIZE + 1]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        // Rounded through ten significant digits, as the hardware's table was
        // generated.
        std::array::from_fn(|i| {
            let x = (std::f64::consts::TAU * (i as f64 / SINE_TABLE_SIZE as f64)).sin();
            format!("{x:.9e}").parse().unwrap_or(x as f32)
        })
    })
}

/// Build the sine table. Main thread only.
pub fn warm_tables() {
    sine_table();
}

/// Phase-accumulating sine LFO with output in `[0, 1]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SineOscillator {
    phase: f32,
}

impl SineOscillator {
    #[inline]
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Advance by `frequency` cycles and return the new value.
    #[inline]
    pub fn render(&mut self, frequency: f32) -> f32 {
        self.phase += frequency;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let table = sine_table();
        let index = self.phase * SINE_TABLE_SIZE as f32;
        let integral = index as usize;
        let fractional = index - integral as f32;
        let a = table[integral];
        let b = table[integral + 1];
        0.5 + 0.5 * (a + (b - a) * fractional)
    }
}

/// Half of the symmetric 48-tap FIR stmlib uses to decimate by two
/// (`scipy.signal.remez(48, [0, 0.1625, 0.25, 0.5], [1, 0])`). Interpolation
/// uses the same response at twice the gain.
#[allow(clippy::excessive_precision)]
const SRC2_HALF: [f32; 24] = [
    -2.143728787e-04,
    -4.113243578e-04,
    3.550026182e-04,
    1.123588805e-03,
    2.013790146e-04,
    -2.135889966e-03,
    -1.946536068e-03,
    2.421957824e-03,
    5.099674271e-03,
    -4.847193956e-04,
    -8.638296788e-03,
    -5.137744633e-03,
    1.000597441e-02,
    1.464501735e-02,
    -5.419339385e-03,
    -2.582247866e-02,
    -9.106955536e-03,
    3.335521514e-02,
    3.702172341e-02,
    -2.802414194e-02,
    -8.416883646e-02,
    -1.192656678e-02,
    1.955030744e-01,
    3.837045956e-01,
];

const SRC2_TAPS: usize = 48;

#[inline]
fn src2_tap(k: usize) -> f32 {
    if k < SRC2_TAPS / 2 {
        SRC2_HALF[k]
    } else {
        SRC2_HALF[SRC2_TAPS - 1 - k]
    }
}

/// 2:1 decimator for one channel: call [`Self::push`] twice, then read.
#[derive(Clone, Copy, Debug)]
pub struct Decimator2 {
    history: [f32; SRC2_TAPS],
    pos: usize,
}

impl Default for Decimator2 {
    fn default() -> Self {
        Self {
            history: [0.0; SRC2_TAPS],
            pos: 0,
        }
    }
}

impl Decimator2 {
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.pos = (self.pos + SRC2_TAPS - 1) % SRC2_TAPS;
        self.history[self.pos] = x;
    }

    #[inline]
    pub fn output(&self) -> f32 {
        // Summed from the oldest tap, matching stmlib's rounding.
        let mut acc = 0.0;
        for k in (0..SRC2_TAPS).rev() {
            acc += self.history[(self.pos + k) % SRC2_TAPS] * src2_tap(k);
        }
        acc
    }
}

/// 1:2 interpolator for one channel: each input sample yields two outputs.
#[derive(Clone, Copy, Debug)]
pub struct Interpolator2 {
    history: [f32; SRC2_TAPS / 2],
    pos: usize,
}

impl Default for Interpolator2 {
    fn default() -> Self {
        Self {
            history: [0.0; SRC2_TAPS / 2],
            pos: 0,
        }
    }
}

impl Interpolator2 {
    #[inline]
    pub fn process(&mut self, x: f32) -> [f32; 2] {
        const N: usize = SRC2_TAPS / 2;
        self.pos = (self.pos + N - 1) % N;
        self.history[self.pos] = x;
        let mut out = [0.0; 2];
        for (phase, y) in out.iter_mut().enumerate() {
            let mut acc = 0.0;
            for i in (0..N).rev() {
                acc += self.history[(self.pos + i) % N] * (2.0 * src2_tap(phase + 2 * i));
            }
            *y = acc;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limiter_lets_no_sample_above_its_peak() {
        let mut limiter = Limiter::default();
        limiter.init(1.0 / (48_000.0 * 4.0));
        for n in 0..4800 {
            let x = if n < 100 {
                0.1
            } else {
                200.0 * ((n % 7) as f32 - 3.0)
            };
            let y = limiter.process_mono(x);
            assert!(y.abs() <= 1.0 + 1e-6, "{n}: {x} → {y}");
        }
    }
}
