//! Scalar waveshaper kernels ported from Surge XT's `sst-waveshapers`.
//!
//! Each function mirrors a Surge XT shaper. The stateless nonlinearities live here
//! as free functions; the stateful ones (ADAA rectifiers/folders, DC-blocked
//! harmonics/fuzz) expose their per-sample kernel math here and are wired to
//! their state in the owning module.
//!
//! Ported from https://github.com/surge-synthesizer/sst-waveshapers (GPL-3.0-or-later).

use std::f32::consts::PI;
use std::sync::LazyLock;

/// Sign as Surge XT defines it for shapers: `x >= 0 ? +1 : -1` (zero counts as +1).
#[inline]
fn sgn(x: f32) -> f32 {
    if x >= 0.0 { 1.0 } else { -1.0 }
}

/// Clamp to the unit interval `[-1, 1]`.
#[inline]
fn unit_clip(x: f32) -> f32 {
    x.clamp(-1.0, 1.0)
}

// ============ Saturators ============

/// Padé tanh approximation on an already-driven input: `x·(27 + x²)/(27 + 9x²)`,
/// clamped to ±1. This is Surge XT's `TANH` body (closer to ideal than plain tanh).
#[inline]
pub fn tanh_driven(x: f32) -> f32 {
    let xx = x * x;
    unit_clip(x * (27.0 + xx) / (27.0 + 9.0 * xx))
}

/// `TANH`: soft saturation.
#[inline]
pub fn tanh(x: f32, drive: f32) -> f32 {
    tanh_driven(x * drive)
}

/// `CLIP`: hard clip at ±1 after drive.
#[inline]
pub fn clip(x: f32, drive: f32) -> f32 {
    unit_clip(x * drive)
}

/// `ZAMSAT` ("Medium"): clip, then `2x − sgn(x)·x²`.
#[inline]
pub fn zamsat(x: f32, drive: f32) -> f32 {
    let x = clip(x, drive);
    2.0 * x - sgn(x) * (x * x)
}

/// `OJD`: piecewise saturation with a soft knee near ±1.
#[inline]
pub fn ojd(x: f32, drive: f32) -> f32 {
    // 1 / (4·(1 − knee)) for the two knee regions.
    const DEN_LOW: f32 = 1.0 / (4.0 * (1.0 - 0.3));
    const DEN_HIGH: f32 = 1.0 / (4.0 * (1.0 - 0.9));

    let x = x * drive;
    if x <= -1.7 {
        -1.0
    } else if x < -0.3 {
        let xl = x + 0.3;
        xl + DEN_LOW * (xl * xl) - 0.3
    } else if x <= 0.9 {
        x
    } else if x < 1.1 {
        let xh = x - 0.9;
        xh - DEN_HIGH * (xh * xh) + 0.9
    } else {
        1.0
    }
}

/// `shafted_tanh(x) = (eˣ − e^(−1.2x)) / (eˣ + e^(−x))` — the asymmetric core.
#[inline]
fn shafted_tanh(x: f32) -> f32 {
    let ex = x.exp();
    (ex - (x * -1.2).exp()) / (ex + (-x).exp())
}

/// `shafted_tanh(0.5)`, the DC offset removed to keep the asym curve through 0.
const SHAFTED_TANH_HALF: f32 = 0.487_710_32;

/// `ASYM`: asymmetric saturation. Surge XT tabulates `shafted_tanh(x+0.5) −
/// shafted_tanh(0.5)` over the LUT's input span; computed directly here. The
/// input is clamped to the table's domain so it saturates rather than diverging.
#[inline]
pub fn asym(x: f32, drive: f32) -> f32 {
    let d = (x * drive).clamp(-16.0, 15.9375);
    shafted_tanh(d + 0.5) - SHAFTED_TANH_HALF
}

// ============ Effects ============

/// `DIGI`: sample quantization. `drive` sets the step; higher drive = coarser.
#[inline]
pub fn digital(x: f32, drive: f32) -> f32 {
    let inv = 1.0 / drive;
    // round-to-nearest ties-to-even matches the reference `cvtps_epi32`.
    let a = (0.5 + inv * 16.0 * x).round_ties_even();
    drive * 0.0625 * (a - 0.5)
}

/// `SINUS`: sine waveshaper. `fold = false` clamps to one period (Surge XT clips the
/// LUT index); `fold = true` lets it wrap, folding the signal through the sine.
/// +6 dB of drive traverses the full sine.
#[inline]
pub fn sine_shaper(x: f32, drive: f32, fold: bool) -> f32 {
    let d = x * drive;
    let phase = if fold { d } else { d.clamp(-2.0, 2.0) };
    (phase * (PI * 0.5)).sin()
}

// ============ Harmonic (Chebyshev) ============

/// Chebyshev polynomials `T₂..T₅` on a value already bound to `[-1, 1]`.
#[inline]
pub fn cheb2(x: f32) -> f32 {
    2.0 * (x * x) - 1.0
}
#[inline]
pub fn cheb3(x: f32) -> f32 {
    let x2 = x * x;
    (4.0 * x2 - 3.0) * x
}
#[inline]
pub fn cheb4(x: f32) -> f32 {
    let x2 = x * x;
    8.0 * (x2 * (x2 - 1.0)) + 1.0
}
#[inline]
pub fn cheb5(x: f32) -> f32 {
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;
    // Grouped 16x⁵ + (−20x³ + 5x) to match Surge XT's f32 addition association.
    16.0 * x5 + (-20.0 * x3 + 5.0 * x)
}

/// `CHEBY_CORE` up to its DC blocker: clamp to ±1, apply the kernel. The caller
/// DC-blocks the result and passes it through [`tanh`].
#[inline]
pub fn cheby_bound(x: f32, kernel: fn(f32) -> f32) -> f32 {
    kernel(unit_clip(x))
}

/// Evaluate `Σ wᵢ·Tᵢ(x)` (a Chebyshev series) via the recurrence
/// `Tₙ = 2x·Tₙ₋₁ − Tₙ₋₂`, matching Surge XT's `ChebSeries::eval`.
#[inline]
pub fn cheb_series(x: f32, weights: &[f32]) -> f32 {
    let mut t_prev = 1.0;
    let mut t_n = x;
    let mut res = weights[0] + weights[1] * x;
    for &w in &weights[2..] {
        let next = 2.0 * t_n * x - t_prev;
        t_prev = t_n;
        t_n = next;
        res += w * t_n;
    }
    res
}

/// Input scale shared by the additive-harmonic modes before `tanh`.
pub const ADDITIVE_SCALE: f32 = 0.66;

// ============ Rectifiers (ADAA kernels → (F, antiderivative)) ============

/// `x > 0 ? x : 0`, with antiderivative `F²/2`.
#[inline]
pub fn posrect_kernel(x: f32) -> (f32, f32) {
    let f = x.max(0.0);
    (f, 0.5 * f * f)
}
/// `x < 0 ? x : 0`, with antiderivative `F²/2`.
#[inline]
pub fn negrect_kernel(x: f32) -> (f32, f32) {
    let f = x.min(0.0);
    (f, 0.5 * f * f)
}
/// Full-wave `|x|`, with antiderivative `sgn(x)·x²/2`.
#[inline]
pub fn fwrect_kernel(x: f32) -> (f32, f32) {
    let s = sgn(x);
    let f = s * x;
    (f, f * x * 0.5)
}
/// Soft rectifier `2·sgn(x)·x − 1`, with antiderivative `sgn(x)·x² − x`.
#[inline]
pub fn softrect_kernel(x: f32) -> (f32, f32) {
    let s = sgn(x);
    let f = 2.0 * (s * x) - 1.0;
    let ad = s * (x * x) - x;
    (f, ad)
}

// ============ Wavefolders ============

/// `SoftOneFold`: `x·drive / (0.4 + 0.7·(x·drive)²)`.
#[inline]
pub fn soft_one_fold(x: f32, drive: f32) -> f32 {
    let y = x * drive;
    y / (0.4 + 0.7 * (y * y))
}

/// `LINFOLD`: Vital-derived linear (triangle) fold.
#[inline]
pub fn linear_fold(x: f32, drive: f32) -> f32 {
    // Prescale into the triangle's phase, then take the fractional part (mod 1).
    let x = (x * drive) * 0.25 + 0.75;
    let e = x.round_ties_even();
    let a = x - e;
    let a = if a < 0.0 { a + 1.0 } else { a }; // wrap to [0, 1)
    let a = a * -4.0 + 2.0;
    a.abs() - 1.0
}

/// Piecewise-linear folder with per-segment slopes and antiderivative
/// intercepts, mirroring Surge XT's `FolderADAA`. Built once at compile time.
pub struct Folder<const PTS: usize> {
    xs: [f32; PTS],
    ys: [f32; PTS],
    slopes: [f32; PTS],
    intercepts: [f32; PTS],
}

/// Precompute slopes and antiderivative intercepts for a folder's breakpoints.
const fn build_folder<const PTS: usize>(xs: [f32; PTS], ys: [f32; PTS]) -> Folder<PTS> {
    let mut slopes = [0.0f32; PTS];
    let mut intercepts = [0.0f32; PTS];
    intercepts[0] = -xs[0] * ys[0];
    let mut i = 0;
    while i < PTS - 1 {
        let dx = xs[i + 1] - xs[i];
        slopes[i] = (ys[i + 1] - ys[i]) / dx;
        let v_left = slopes[i] * dx * dx / 2.0 + ys[i] * xs[i + 1] + intercepts[i];
        let v_right = ys[i + 1] * xs[i + 1];
        intercepts[i + 1] = -v_right + v_left;
        i += 1;
    }
    Folder {
        xs,
        ys,
        slopes,
        intercepts,
    }
}

impl<const PTS: usize> Folder<PTS> {
    /// Evaluate the fold and its antiderivative at `x`. Segments are ordered, so
    /// the last one whose left breakpoint `x ≥ xs[i]` is the containing segment.
    /// Outside the breakpoint span `[xs[0], xs[PTS-1])` both are 0, matching Surge
    /// XT's range-masked sum (no segment selected → 0).
    #[inline]
    pub fn evaluate(&self, x: f32) -> (f32, f32) {
        if x < self.xs[0] || x >= self.xs[PTS - 1] {
            return (0.0, 0.0);
        }
        let seg = |i: usize| -> (f32, f32) {
            let ox = x - self.xs[i];
            let slope = self.slopes[i];
            let val = slope * ox + self.ys[i];
            // Grouped as A + (B + C) to match Surge XT's f32 addition association.
            let ad = (ox * ox) * (slope * 0.5) + (self.ys[i] * x + self.intercepts[i]);
            (val, ad)
        };
        let (mut f, mut ad) = seg(0);
        for i in 1..PTS - 1 {
            if x >= self.xs[i] {
                let (v, a) = seg(i);
                f = v;
                ad = a;
            }
        }
        (f, ad)
    }
}

pub static SINGLE_FOLD: Folder<4> = build_folder([-10.0, -0.7, 0.7, 10.0], [-1.0, 1.0, -1.0, 1.0]);

pub static DUAL_FOLD: Folder<8> = build_folder(
    [-10.0, -3.0, -1.0, -0.3, 0.3, 1.0, 3.0, 10.0],
    [-1.0, -0.9, 1.0, -1.0, 1.0, -1.0, 0.9, 1.0],
);

// Full-precision breakpoints from Surge XT's westCoastFoldADAA (some are slightly
// asymmetric); truncating these to ~7 sig figs shifts f32 values by 1–2 ULP.
pub static WESTCOAST_FOLD: Folder<14> = build_folder(
    [
        -10.0,
        -2.0,
        -1.091_909_190_919_091_9,
        -0.815_881_588_158_816,
        -0.598_659_865_986_598_7,
        -0.359_835_983_598_359_7,
        -0.119_811_981_198_119_71,
        0.119_811_981_198_119_71,
        0.359_835_983_598_359_7,
        0.598_659_865_986_598_7,
        0.815_881_588_158_815_7,
        1.091_909_190_919_091_9,
        2.0,
        10.0,
    ],
    [
        1.0,
        0.9,
        -0.679_765_619_488_133,
        0.530_965_997_227_062_5,
        -0.625_550_663_174_425_1,
        0.599_179_917_991_798_7,
        -0.599_059_905_990_598_6,
        0.599_059_905_990_598_6,
        -0.599_179_917_991_798_7,
        0.625_550_663_174_425_1,
        -0.530_965_997_227_064_2,
        0.679_765_619_488_133,
        -0.9,
        -1.0,
    ],
);

// ============ Trigonometric ============

/// `Sin+x`: `x − sin(π·x)` on the clipped, driven input.
#[inline]
pub fn sin_plus_x(x: f32, drive: f32) -> f32 {
    let c = clip(x, drive);
    c - (c * PI).sin()
}
/// `Sin Nx + x`, bounded by `(1 − |x|)`.
#[inline]
pub fn sin_nx_plus_x_bound(x: f32, drive: f32, n: f32) -> f32 {
    let c = clip(x, drive);
    let z = 1.0 - c.abs();
    z * (c * (PI * n)).sin() + c
}
/// `sin(π·N·x)` — N full cycles across the clipped input.
#[inline]
pub fn sin_nx(x: f32, drive: f32, n: f32) -> f32 {
    let c = clip(x, drive);
    (c * (PI * n)).sin()
}
/// `(1 − |x|)·sin(π·N·x)` — N cycles tapered to the edges.
#[inline]
pub fn sin_nx_bound(x: f32, drive: f32, n: f32) -> f32 {
    let c = clip(x, drive);
    let z = 1.0 - c.abs();
    z * (c * (PI * n)).sin()
}

// ============ Fuzz (frozen, seeded tables) ============
//
// Surge XT fuzzes are static nonlinearities defined by a table filled with a seeded
// PRNG: the randomness is baked into the curve, not generated per sample, so the
// table must be reproduced deterministically. Surge XT's `std::uniform_real_distribution`
// is implementation-defined and cannot be reproduced bit-for-bit across standard
// libraries, so we use the same `minstd` engine (seed 2112) with a direct uniform
// map — deterministic, reproducible, and the same fuzz character.

/// `std::minstd_rand`: `state ← state · 48271 mod (2³¹ − 1)`.
struct Minstd {
    state: u32,
}

impl Minstd {
    fn new() -> Self {
        Self { state: 2112 }
    }
    /// Next uniform value in `[-range, range)`.
    #[inline]
    fn uniform(&mut self, range: f32) -> f32 {
        self.state = ((self.state as u64 * 48271) % 2_147_483_647) as u32;
        let u = self.state as f32 / 2_147_483_647.0; // [0, 1)
        (u * 2.0 - 1.0) * range
    }
}

/// A fuzz LUT of `N + 1` samples spanning input `[-1, 1]`.
struct FuzzTable {
    data: Vec<f32>,
    n: usize,
}

impl FuzzTable {
    fn build(n: usize, mut f: impl FnMut(f32, &mut Minstd) -> f32) -> Self {
        let mut rng = Minstd::new();
        let dx = 2.0 / n as f32;
        let data = (0..=n).map(|i| f(i as f32 * dx - 1.0, &mut rng)).collect();
        Self { data, n }
    }

    /// Interpolated lookup for a value in `[-1, 1]`, per Surge XT's `WS_PM1_LUT`:
    /// map to `[0, n]`; the segment index is round-to-nearest-even of the clamped
    /// coordinate (Surge XT's `cvtps_epi32`), while the interpolation fraction is
    /// taken against the UN-clamped coordinate; then lerp `(1−frac)·a + frac·b`.
    #[inline]
    fn lookup(&self, input: f32) -> f32 {
        let half = self.n as f32 * 0.5;
        let x = input * half + half;
        let e = x.clamp(0.0, (self.n - 1) as f32).round_ties_even() as usize;
        let frac = x - e as f32;
        let a = self.data[e];
        let b = self.data[e + 1];
        a + (b - a) * frac
    }
}

/// `FuzzTable<scale>`: `x·(1 − range) + uniform(±range)`, `range = 0.1·scale`.
fn fuzz_table(scale: f32) -> FuzzTable {
    let range = 0.1 * scale;
    FuzzTable::build(1024, move |x, rng| x * (1.0 - range) + rng.uniform(range))
}

static FUZZ_1: LazyLock<FuzzTable> = LazyLock::new(|| fuzz_table(1.0));
static FUZZ_3: LazyLock<FuzzTable> = LazyLock::new(|| fuzz_table(3.0));
static FUZZ_CTR: LazyLock<FuzzTable> = LazyLock::new(|| {
    // Centred fuzz: a Gaussian bump of noise around 0.
    FuzzTable::build(2048, |x, rng| {
        let g = (-x * x * 20.0).exp();
        x + g * rng.uniform(1.0)
    })
});
static FUZZ_EDGE: LazyLock<FuzzTable> = LazyLock::new(|| {
    // Soft-edge fuzz: noise weighted toward the extremes by x⁴.
    FuzzTable::build(2048, |x, rng| {
        let g = x * x * x * x;
        0.85 * x + 0.15 * g * rng.uniform(1.0)
    })
});

/// Which frozen fuzz table a mode uses.
#[derive(Clone, Copy)]
pub enum FuzzKind {
    Standard,
    Heavy,
    Center,
    Edge,
}

/// Look up one sample through the selected fuzz table.
#[inline]
pub fn fuzz_lookup(kind: FuzzKind, x: f32) -> f32 {
    let table: &FuzzTable = match kind {
        FuzzKind::Standard => &FUZZ_1,
        FuzzKind::Heavy => &FUZZ_3,
        FuzzKind::Center => &FUZZ_CTR,
        FuzzKind::Edge => &FUZZ_EDGE,
    };
    table.lookup(x)
}

/// Force the fuzz tables to build on the main thread (call from `init`) so their
/// one-time fill never runs on the audio thread.
pub fn prime_fuzz_tables() {
    LazyLock::force(&FUZZ_1);
    LazyLock::force(&FUZZ_3);
    LazyLock::force(&FUZZ_CTR);
    LazyLock::force(&FUZZ_EDGE);
}
