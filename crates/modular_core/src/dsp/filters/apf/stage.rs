//! Two-pole allpass stages: Simper state-variable filters, and the
//! coefficient math that tunes them.

/// Stage Q at resonance 5; resonance 0 is Q 0.5.
pub(super) const MAX_Q: f32 = 20.0;

/// Octaves above its cutoff a gliding stage starts its even descent from.
const GLIDE_OCTAVES: f32 = 10.0;

/// Bilinear prewarp `tan(π f / fs)` for `log2(f / fs)`, with `f` kept
/// between `fs / 2^18` and 0.49 fs so it stays positive and finite.
pub(super) fn prewarp(octaves_re_fs: f32) -> f32 {
    let freq = octaves_re_fs.clamp(-18.0, 0.49f32.log2()).exp2();
    (std::f32::consts::PI * freq).tan()
}

/// Damping `1 / Q` of a stage at `resonance`, Q rising exponentially from
/// 0.5 at 0 to `MAX_Q` at 5.
pub(super) fn damping(resonance: f32) -> f32 {
    2.0 * (2.0 * MAX_Q).powf(-resonance.clamp(0.0, 5.0) / 5.0)
}

/// `1 / g` of a stage gliding in from the top of the spectrum toward
/// prewarp `g`: 0 (at Nyquist, where a stage passes everything unchanged)
/// at `amount` 0, then down at a roughly even rate in octaves to `g` at 1.
pub(super) fn glide(g: f32, amount: f32) -> f32 {
    amount * (GLIDE_OCTAVES * (amount - 1.0)).exp2() / g
}

#[derive(Clone, Copy, Default)]
pub(super) struct StageCoeffs {
    /// Damping, `1 / Q`.
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    /// The allpass output `v0 - 2k·v1` expanded over the state: `m·v0 +
    /// p·ic2 - q·ic1`.
    m: f32,
    p: f32,
    q: f32,
}

impl StageCoeffs {
    pub(super) fn new(g: f32, k: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self::from_parts(k, a1, a2, g * a2)
    }

    /// Coefficients for reciprocal prewarp `t = 1 / g`, which stay finite
    /// as the cutoff reaches Nyquist (`t` = 0).
    pub(super) fn from_reciprocal(t: f32, k: f32) -> Self {
        let d = 1.0 / (t * t + 1.0 + k * t);
        Self::from_parts(k, t * t * d, t * d, d)
    }

    fn from_parts(k: f32, a1: f32, a2: f32, a3: f32) -> Self {
        Self {
            k,
            a1,
            a2,
            a3,
            m: 1.0 - 2.0 * k * a2,
            p: 2.0 * k * a2,
            q: 2.0 * k * a1,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Stage {
    ic1: f32,
    ic2: f32,
}

impl Stage {
    /// The state of a stage that has been passing a steady `x`: its lowpass
    /// integrator holds it.
    pub(super) fn settle(&mut self, x: f32) {
        *self = Self { ic1: 0.0, ic2: x };
    }

    /// One sample, returning the bandpass response `v1`. `v0` must be
    /// finite: the channel sanitizes its input once, and each stage stays
    /// finite because its coefficients and state are.
    #[inline(always)]
    fn tick(&mut self, c: &StageCoeffs, v0: f32) -> f32 {
        let v3 = v0 - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        v1
    }

    /// Allpass response. Expanded over the state, only one multiply-add
    /// waits on `v0`, which shortens a chain's sample-to-sample latency.
    #[inline(always)]
    pub(super) fn allpass(&mut self, c: &StageCoeffs, v0: f32) -> f32 {
        let y = c.m * v0 + (c.p * self.ic2 - c.q * self.ic1);
        self.tick(c, v0);
        y
    }

    /// Bandpass response, unity at its center.
    #[inline(always)]
    pub(super) fn bandpass(&mut self, c: &StageCoeffs, v0: f32) -> f32 {
        c.k * self.tick(c, v0)
    }
}
