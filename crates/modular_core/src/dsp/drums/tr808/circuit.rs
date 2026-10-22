//! Building blocks shared by the TR-808 voice models: the common trigger
//! pulse, the noise generator, and discretised analog filters.

use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::voice::{FADE_SECS, quiet_fall, samples};
use crate::dsp::utils::rng::LcgRng;

/// Length of the 808's common trigger pulse, seconds.
pub const PULSE_SECS: f32 = 1.0e-3;
/// Trigger pulse voltage without and with full accent. The 808 reaches 14 V
/// at full accent; 8 V keeps accented hits at twice the level of plain ones.
pub const PULSE_V: f32 = 4.0;
pub const ACCENT_V: f32 = 4.0;

/// The trigger pulse's voltage for a hit with `accent`, 0–1.
pub fn pulse_volts(accent: f32) -> f32 {
    PULSE_V + ACCENT_V * accent
}

/// Output swing of the voices' µPC4558 op-amps on the ±15 V rails.
pub const VSAT: f32 = 13.5;
/// Forward drop of a conducting 1S2473 silicon diode.
pub const DIODE_DROP: f32 = 0.71;

/// ln(1000): converts a time to −60 dB into a decay rate.
pub const T60_TO_RATE: f32 = 6.907_755;

/// Below this every state of a voice counts as settled.
pub const SILENCE: f32 = 1.0e-6;

/// The 808's white noise generator: a reverse-biased transistor junction
/// amplified to 130 mV rms across the audio band.
#[derive(Clone, Copy, Default)]
pub struct WhiteNoise {
    rng: LcgRng,
    seeded: bool,
}

/// Noise bandwidth the 130 mV rms level is specified over, Hz.
const NOISE_BAND_HZ: f32 = 20_000.0;
const NOISE_RMS: f32 = 0.13;

impl WhiteNoise {
    /// Seeds the generator unless it already is. The generator rides patch
    /// updates with its channel, so a carried-over channel keeps its
    /// sequence and a channel added by an update gets its own.
    pub fn seed(&mut self, base: u64, stream: usize) {
        if !self.seeded {
            self.rng.seed(base, stream);
            self.seeded = true;
        }
    }

    /// Scale from a uniform `[-1, 1)` sample to the generator's voltage, so
    /// its spectral density is the same at every sample rate.
    pub fn scale(sample_rate: f32) -> f32 {
        NOISE_RMS * (0.5 * sample_rate / NOISE_BAND_HZ).sqrt() * 3f32.sqrt()
    }

    #[inline]
    pub fn next(&mut self, scale: f32) -> f32 {
        self.rng.next_bipolar() * scale
    }
}

/// One trapezoidal step of `u' = A·u + (0, b2)` with `A` row-major.
#[inline]
pub fn trapezoid<S: Real<Elem = f32>>(u: (S, S), a: [S; 4], b2: S, dt: f32) -> (S, S) {
    let h = 0.5 * dt;
    let one = c::<S>(1.0);
    let (m11, m12, m21, m22) = (one - a[0] * h, a[1] * -h, a[2] * -h, one - a[3] * h);
    let r1 = u.0 + (a[0] * u.0 + a[1] * u.1) * h;
    let r2 = u.1 + (a[2] * u.0 + a[3] * u.1) * h + b2 * dt;
    let det = m11 * m22 - m12 * m21;
    ((m22 * r1 - m12 * r2) / det, (m11 * r2 - m21 * r1) / det)
}

/// Second-order section in transposed direct form II.
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
}

/// The running state of a [`Biquad`].
#[derive(Clone, Copy, Default)]
pub struct BiquadState<L: Lanes = Mono> {
    s1: L::S,
    s2: L::S,
}

lanewise!(BiquadState { s1, s2 });

impl<L: Lanes> BiquadState<L> {
    pub fn settled(&self) -> L::SM {
        self.s1.abs().lt(c(SILENCE)) & self.s2.abs().lt(c(SILENCE))
    }

    /// `a` in the lanes of `mask`, `b` in the others.
    #[inline]
    pub fn select(mask: L::SM, a: Self, b: Self) -> Self {
        Self {
            s1: L::S::select(mask, a.s1, b.s1),
            s2: L::S::select(mask, a.s2, b.s2),
        }
    }
}

impl Biquad {
    /// Bilinear transform of `(b2·s² + b1·s + b0) / (a2·s² + a1·s + a0)`,
    /// prewarped to be exact at `match_hz`.
    pub fn analog(b: [f32; 3], a: [f32; 3], match_hz: f32, sample_rate: f32) -> Self {
        let w = std::f32::consts::TAU * match_hz.min(0.45 * sample_rate);
        let k = w / (0.5 * w / sample_rate).tan();
        let (k2, [b0, b1, b2], [a0, a1, a2]) = (k * k, b, a);
        let norm = a2 * k2 + a1 * k + a0;
        Self {
            b: [
                (b2 * k2 + b1 * k + b0) / norm,
                2.0 * (b0 - b2 * k2) / norm,
                (b2 * k2 - b1 * k + b0) / norm,
            ],
            a: [2.0 * (a0 - a2 * k2) / norm, (a2 * k2 - a1 * k + a0) / norm],
        }
    }

    /// Unity-gain Sallen-Key low-pass: series resistors `r1`, `r2`, feedback
    /// capacitor `c1` and capacitor `c2` to ground.
    pub fn sallen_key_low(r1: f32, r2: f32, c1: f32, c2: f32, sample_rate: f32) -> Self {
        let a2 = r1 * r2 * c1 * c2;
        let hz = 1.0 / (std::f32::consts::TAU * a2.sqrt());
        Self::analog([1.0, 0.0, 0.0], [1.0, c2 * (r1 + r2), a2], hz, sample_rate)
    }

    /// Unity-gain Sallen-Key high-pass: series capacitors `c1`, `c2`,
    /// feedback resistor `r1` and resistor `r2` to ground.
    pub fn sallen_key_high(c1: f32, c2: f32, r1: f32, r2: f32, sample_rate: f32) -> Self {
        let a2 = r1 * r2 * c1 * c2;
        let hz = 1.0 / (std::f32::consts::TAU * a2.sqrt());
        Self::analog([0.0, 0.0, a2], [1.0, r1 * (c1 + c2), a2], hz, sample_rate)
    }

    #[inline]
    pub fn process<L: Lanes>(&self, state: &mut BiquadState<L>, x: L::S) -> L::S {
        let y = x * self.b[0] + state.s1;
        state.s1 = (x * self.b[1] - y * self.a[0] + state.s2).sanitize();
        state.s2 = (x * self.b[2] - y * self.a[1]).sanitize();
        y
    }
}

/// An op-amp with a driven non-inverting input and a bridged-T in its
/// feedback path: arm `c_a` from the inverting input to the junction, arm
/// `c_b` from the junction to the output, `r_bridge` across both, a shunt from
/// the junction to ground, and the inverted output fed back into the junction
/// through `g_fb`.
#[derive(Clone, Copy, Default)]
pub struct BridgedT<L: Lanes = Mono> {
    pub c_a: L::S,
    pub c_b: L::S,
    pub r_bridge: L::S,
    pub g_fb: L::S,
}

lanewise!(BridgedT {
    c_a,
    c_b,
    r_bridge,
    g_fb
});

/// The voltages across a [`BridgedT`]'s arms.
#[derive(Clone, Copy, Default)]
pub struct Arms<L: Lanes = Mono> {
    pub a: L::S,
    pub b: L::S,
}

lanewise!(Arms { a, b });

impl<L: Lanes> Arms<L> {
    pub fn settled(&self) -> L::SM {
        self.a.abs().lt(c(SILENCE)) & self.b.abs().lt(c(SILENCE))
    }
}

impl<L: Lanes> BridgedT<L> {
    /// One trapezoidal step from `arms` with the input at `vp` and a shunt
    /// current `i_shunt + g_shunt·junction`, returning the output and the
    /// new arms. While the op-amp is linear its inverting input follows `vp`,
    /// so junction = vp − a and output = junction − b; past ±[`VSAT`] the
    /// output is pinned and the inverting input floats, so
    /// junction = output + b.
    #[inline]
    pub fn step(
        &self,
        arms: Arms<L>,
        vp: L::S,
        g_shunt: L::S,
        i_shunt: L::S,
        dt: f32,
    ) -> (L::S, Arms<L>) {
        let BridgedT {
            c_a,
            c_b,
            r_bridge,
            g_fb,
        } = *self;
        let a = c::<L::S>(1.0) / (r_bridge * c_a);
        let gb = c::<L::S>(1.0) / r_bridge;
        let u = (arms.a, arms.b);
        let linear = trapezoid(
            u,
            [
                -a,
                -a,
                (-gb + g_fb * 2.0 + g_shunt) / c_b,
                (-gb + g_fb) / c_b,
            ],
            (-(g_fb * 2.0 + g_shunt) * vp - i_shunt) / c_b,
            dt,
        );
        let out = vp - linear.0 - linear.1;
        let over = out.abs().gt(c(VSAT));
        let (out, (a_new, b_new)) = if over.any() {
            let pinned_out = L::S::select(out.lt(c(0.0)), c(-VSAT), c(VSAT));
            let pinned = trapezoid(
                u,
                [-a, -a, -gb / c_b, (-gb - g_fb - g_shunt) / c_b],
                (-(g_fb * 2.0 + g_shunt) * pinned_out - i_shunt) / c_b,
                dt,
            );
            (
                L::S::select(over, pinned_out, out),
                (
                    L::S::select(over, pinned.0, linear.0),
                    L::S::select(over, pinned.1, linear.1),
                ),
            )
        } else {
            (out, linear)
        };
        (
            out,
            Arms {
                a: a_new.sanitize(),
                b: b_new.sanitize(),
            },
        )
    }
}

impl BridgedT {
    /// Resting frequency with a linear shunt conductance `g_shunt`, Hz.
    pub fn rest_hz(&self, g_shunt: f32) -> f32 {
        let a = 1.0 / (self.r_bridge * self.c_a);
        (a * (self.g_fb + g_shunt) / self.c_b).sqrt() / std::f32::consts::TAU
    }

    /// The ring's decay rate, 1/s: R_bridge's loss through both arms less
    /// what the feedback restores.
    pub fn decay_rate(&self) -> f32 {
        0.5 * (1.0 / (self.r_bridge * self.c_a) + (1.0 / self.r_bridge - self.g_fb) / self.c_b)
    }

    /// The same T with both arms scaled by 1/`r`, ringing `r` times higher,
    /// and the feedback set for a ring of `decay` seconds to −60 dB.
    pub fn tuned(&self, r: f32, decay: f32) -> Self {
        let (c_a, c_b) = (self.c_a / r, self.c_b / r);
        let a = 1.0 / (self.r_bridge * c_a);
        let gb = 1.0 / self.r_bridge;
        let rate = T60_TO_RATE / decay;
        let g_crit = gb + c_b * a;
        Self {
            c_a,
            c_b,
            r_bridge: self.r_bridge,
            g_fb: (gb + c_b * (a - 2.0 * rate)).clamp(0.0, g_crit * 0.999),
        }
    }
}

/// A coupling capacitor from the trigger pulse into a node with resistor
/// `r_shunt` to ground and a diode into the divider `r_series`/`r_ground`
/// that feeds an op-amp input: positive edges pass, negative ones are
/// blocked. The node relaxes within tens of microseconds, so it is stepped
/// in substeps and the op-amp sees each sample's average.
#[derive(Clone, Copy)]
pub struct DiodeCoupler {
    pub c: f32,
    pub r_shunt: f32,
    pub r_series: f32,
    pub r_ground: f32,
}

/// Substeps per sample for a [`DiodeCoupler`].
const COUPLER_SUBSTEPS: usize = 8;

impl DiodeCoupler {
    /// The op-amp input for a node voltage `n`.
    #[inline]
    fn output(&self, n: f32) -> f32 {
        (n - DIODE_DROP).max(0.0) * self.r_ground / (self.r_series + self.r_ground)
    }

    /// Advances the capacitor voltage `v` over one sample with the pulse at
    /// `vtrig`, returning the op-amp input's average.
    #[inline]
    pub fn step(&self, v: &mut f32, vtrig: f32, dt: f32) -> f32 {
        let k = dt / (COUPLER_SUBSTEPS as f32 * self.c);
        let r_div = self.r_series + self.r_ground;
        let g_off = 1.0 / self.r_shunt;
        let g_on = g_off + 1.0 / r_div;
        let (keep_off, keep_on) = (1.0 / (1.0 + k * g_off), 1.0 / (1.0 + k * g_on));
        // With no pulse and the diode blocked, the node only relaxes through
        // R_shunt toward ground, so the diode stays blocked all sample.
        if vtrig == 0.0 && -*v <= DIODE_DROP {
            let keep2 = keep_off * keep_off;
            let keep4 = keep2 * keep2;
            *v *= keep4 * keep4;
            return 0.0;
        }
        let mut sum = 0.0;
        for _ in 0..COUPLER_SUBSTEPS {
            // The capacitor carries the node's current into R_shunt and,
            // past the diode drop, into the divider; backward Euler on
            // C·v' = g·(vtrig − v) + i_off.
            let n = vtrig - *v;
            *v = if n > DIODE_DROP {
                (*v + k * (g_on * vtrig - DIODE_DROP / r_div)) * keep_on
            } else {
                (*v + k * g_off * vtrig) * keep_off
            };
            sum += 0.5 * (self.output(n) + self.output(vtrig - *v));
        }
        sum / COUPLER_SUBSTEPS as f32
    }
}

/// Per-sample-rate timing shared by every voice's channels.
#[derive(Clone, Copy, Default)]
pub struct Timing {
    pub pulse_samples: u32,
    pub fade_samples: u32,
    /// [`quiet_fall`] at the sample rate.
    pub quiet_fall: f32,
}

impl Timing {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            pulse_samples: samples(PULSE_SECS, sample_rate),
            fade_samples: samples(FADE_SECS, sample_rate),
            quiet_fall: quiet_fall(sample_rate),
        }
    }
}
