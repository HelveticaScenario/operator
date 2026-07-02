//! Surge XT's Blackman-windowed sinc interpolation table (`utilities/SincTable.h`),
//! used by the comb's fractional delay-line read.
//!
//! Surge XT's `sinctable` interleaves a per-row delta block for its other interpolators
//! (stride `2·FIRipol_N`); the comb reads only the coefficient half of each row, so
//! this port stores just those (stride [`FIRIPOL_N`]).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use std::sync::LazyLock;

/// Interpolation phases per sample (table rows − 1).
pub const FIRIPOL_M: usize = 256;
/// Sinc taps per interpolation (FIR length).
pub const FIRIPOL_N: usize = 12;
/// Read-position offset: the FIR is centered `FIRIPOL_N/2` taps back.
pub const FIROFFSET: usize = FIRIPOL_N / 2;

/// `sin(πx)/(πx)`.
fn sincf(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
}

/// Surge `symmetric_blackman`.
fn symmetric_blackman(mut i: f64, n: usize) -> f64 {
    let n = n as f64;
    i -= n / 2.0;
    0.42 - 0.5 * (2.0 * std::f64::consts::PI * i / n).cos()
        + 0.08 * (4.0 * std::f64::consts::PI * i / n).cos()
}

/// The `(FIRIPOL_M + 1) × FIRIPOL_N` tap table (row = interpolation phase). Built on
/// first access; [`prime`] forces that onto the main thread.
static SINC_TABLE: LazyLock<Box<[f32]>> = LazyLock::new(|| {
    const CUTOFF: f64 = 0.455;
    let mut table = vec![0.0f32; (FIRIPOL_M + 1) * FIRIPOL_N];
    for j in 0..=FIRIPOL_M {
        for i in 0..FIRIPOL_N {
            let t = -(i as f64) + (FIRIPOL_N as f64 / 2.0) + (j as f64) / (FIRIPOL_M as f64) - 1.0;
            table[j * FIRIPOL_N + i] =
                (symmetric_blackman(t, FIRIPOL_N) * CUTOFF * sincf(CUTOFF * t)) as f32;
        }
    }
    table.into_boxed_slice()
});

/// Build the table now (main thread) so the audio thread never triggers it.
pub fn prime() {
    LazyLock::force(&SINC_TABLE);
}

/// The `FIRIPOL_N` taps for interpolation phase `phase` (0..=`FIRIPOL_M`).
#[inline]
pub fn taps(phase: usize) -> &'static [f32] {
    &SINC_TABLE[phase * FIRIPOL_N..(phase + 1) * FIRIPOL_N]
}
