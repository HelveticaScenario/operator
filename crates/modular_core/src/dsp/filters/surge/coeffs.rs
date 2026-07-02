//! Surge `FilterCoefficientMaker` port (scalar): coefficient helpers shared by the
//! `$unstable.filter.*` kernels — the resonance maps, the state-variable builder, and
//! the normalized-lattice / coupled-form packers that turn biquad `a`/`b` coefficients
//! into the register layout the `IIR*` process kernels expect.
//!
//! All builders return the raw target coefficient array `N`; the engine owns the
//! `FromDirect` glide. Frequencies are in semitones relative to A440 (Surge XT's unit).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use std::f32::consts::PI;

use super::filter_core::N_COEFFS;

/// Surge `FilterSubType` integer constants for the vember biquad family. Kept as the
/// raw ints so the coefficient switches read identically to the C++.
pub const ST_STANDARD: i32 = 0;
pub const ST_DRIVEN: i32 = 1;
pub const ST_CLEAN: i32 = 2;
pub const ST_MEDIUM: i32 = 3;
/// Surge `st_bp12_LegacyDriven` / `st_bp12_LegacyClean` — bandpass-only subtypes.
pub const ST_BP_LEGACY_DRIVEN: i32 = 3;
pub const ST_BP_LEGACY_CLEAN: i32 = 4;

/// `10^(dB/20)`.
#[inline]
fn db_to_linear(x: f64) -> f64 {
    10.0_f64.powf(0.05 * x)
}

/// `440 · 2^(freq_semi / 12)` Hz.
#[inline]
pub fn note_to_hz(freq_semi: f32) -> f32 {
    440.0 * (freq_semi / 12.0).exp2()
}

/// Surge `note_to_omega_ignoring_tuning`: `(sin, cos)` of the digital angular
/// frequency `2π · min(0.5, f/rate)`.
#[inline]
pub fn note_to_omega(freq_semi: f32, rate: f32) -> (f32, f32) {
    let arg = 2.0 * PI * (note_to_hz(freq_semi) / rate).min(0.5);
    (arg.sin(), arg.cos())
}

/// Clamp the pitch (semitones from A440) to Surge XT's supported range.
#[inline]
pub fn bound_freq(freq_semi: f32) -> f32 {
    freq_semi.clamp(-55.0, 75.0)
}

/// Resonance-parameter warp shared by `freq`-dependent drives (`(freq - 58) · 0.05`).
#[inline]
fn reso_freq_comp(reso: f64, freq: f64) -> f64 {
    reso * (1.0 - ((freq - 58.0) * 0.05).max(0.0)).max(0.0)
}

/// Surge `Map2PoleResonance`: resonance (0..1) → the biquad `alpha` scale for a
/// 2-pole section. Only the Driven / Clean subtypes reach this (Standard is SVF).
#[inline]
pub fn map_2pole_resonance(reso: f64, freq: f64, subtype: i32) -> f64 {
    match subtype {
        ST_MEDIUM => {
            let reso = reso_freq_comp(reso, freq);
            0.99 - 1.0 * (1.0 - (1.0 - reso) * (1.0 - reso)).clamp(0.0, 1.0)
        }
        ST_DRIVEN => {
            let reso = reso_freq_comp(reso, freq);
            1.0 - 1.05 * (1.0 - (1.0 - reso) * (1.0 - reso)).clamp(0.001, 1.0)
        }
        // st_Clean and default.
        _ => 2.5 - 2.45 * (1.0 - (1.0 - reso) * (1.0 - reso)).clamp(0.0, 1.0),
    }
}

/// Surge `Map4PoleResonance`.
#[inline]
pub fn map_4pole_resonance(reso: f64, freq: f64, subtype: i32) -> f64 {
    match subtype {
        ST_MEDIUM => {
            let reso = reso_freq_comp(reso, freq);
            0.99 - 0.9949 * reso.clamp(0.0, 1.0)
        }
        ST_DRIVEN => {
            let reso = reso_freq_comp(reso, freq);
            1.0 - 1.05 * reso.clamp(0.001, 1.0)
        }
        _ => 2.5 - 2.3 * reso.clamp(0.0, 1.0),
    }
}

/// Surge `resoscale`: gain compensation applied to the biquad numerator by drive.
#[inline]
pub fn resoscale(reso: f32, subtype: i32) -> f32 {
    match subtype {
        ST_MEDIUM => 1.0 - 0.75 * reso * reso,
        ST_DRIVEN => 1.0 - 0.5 * reso * reso,
        ST_CLEAN => 1.0 - 0.25 * reso * reso,
        _ => 1.0,
    }
}

/// Surge `clipscale`: the `G` (clip-gain) fed to the packers, per drive.
#[inline]
pub fn clipscale(freq_semi: f32, subtype: i32) -> f32 {
    match subtype {
        ST_DRIVEN => (1.0 / 64.0) * db_to_linear((freq_semi * 0.55) as f64) as f32,
        ST_CLEAN => 1.0 / 1024.0,
        _ => 0.0,
    }
}

/// Surge `Coeff_SVF`: the state-variable coefficients used by every "Standard"
/// biquad passband. `four_pole` tightens the resonance overshoot for 24 dB.
/// Returns `N = [F1, Q1, ClipDamp, Gain, 0, 0, 0, 0]`.
pub fn coeff_svf(freq_semi: f32, reso: f32, four_pole: bool, rate: f32) -> [f32; N_COEFFS] {
    let f = note_to_hz(freq_semi) as f64;
    let sr_inv = 1.0 / rate as f64;
    let f1 = 2.0 * (PI as f64 * (0.11_f64).min(f * (0.5 * sr_inv))).sin();

    let reso = (reso.clamp(0.0, 1.0)).sqrt() as f64;

    let overshoot = if four_pole { 0.1 } else { 0.15 };
    let mut q1 = 2.0 - reso * (2.0 + overshoot) + f1 * f1 * overshoot * 0.9;
    q1 = q1.min(2.00_f64.min(2.00 - 1.52 * f1));

    let clip_damp = 0.1 * reso * f1;
    let a = 0.65;
    let gain = 1.0 - a * reso;

    let mut c = [0.0f32; N_COEFFS];
    c[0] = f1 as f32;
    c[1] = q1 as f32;
    c[2] = clip_damp as f32;
    c[3] = gain as f32;
    c
}

/// Surge `ToNormalizedLattice`: pack biquad `a`/`b` coefficients into the normalized
/// ladder form the `IIR*Bquad` ("Clean") kernels read. `g` is the clip gain.
pub fn to_normalized_lattice(
    a0inv: f64,
    mut a1: f64,
    mut a2: f64,
    mut b0: f64,
    mut b1: f64,
    mut b2: f64,
    g: f32,
) -> [f32; N_COEFFS] {
    b0 *= a0inv;
    b1 *= a0inv;
    b2 *= a0inv;
    a1 *= a0inv;
    a2 *= a0inv;

    let k1 = a1 / (1.0 + a2);
    let k2 = a2;

    let q1 = (1.0 - k1 * k1).abs().sqrt();
    let q2 = (1.0 - k2 * k2).abs().sqrt();

    let v3 = b2;
    let v2 = (b1 - a1 * v3) / q2;
    let v1 = (b0 - k1 * v2 * q2 - k2 * v3) / (q1 * q2);

    let mut n = [0.0f32; N_COEFFS];
    n[0] = k1 as f32;
    n[1] = k2 as f32;
    n[2] = q1 as f32;
    n[3] = q2 as f32;
    n[4] = v1 as f32;
    n[5] = v2 as f32;
    n[6] = v3 as f32;
    n[7] = g;
    n
}

/// Surge `ToCoupledForm`: pack biquad `a`/`b` coefficients into the coupled state-space
/// form the `IIR*CFCquad` ("Driven") kernels read. `g` is the clip gain.
pub fn to_coupled_form(
    a0inv: f64,
    mut a1: f64,
    mut a2: f64,
    mut b0: f64,
    mut b1: f64,
    mut b2: f64,
    g: f32,
) -> [f32; N_COEFFS] {
    b0 *= a0inv;
    b1 *= a0inv;
    b2 *= a0inv;
    a1 *= a0inv;
    a2 *= a0inv;

    let sq = (a1 * a1 - 4.0 * a2).min(0.0);
    let ar = 0.5 * -a1;
    let ai = (0.5 * (-sq).sqrt()).max(8.0 * 1.192_092_896e-07);

    let bb1 = b1 - a1 * b0;
    let bb2 = b2 - a2 * b0;

    let d = b0;
    let c1 = bb1;
    let c2 = (bb1 * ar + bb2) / ai;

    let mut n = [0.0f32; N_COEFFS];
    n[0] = ar as f32;
    n[1] = ai as f32;
    n[2] = 1.0;
    n[4] = c1 as f32;
    n[5] = c2 as f32;
    n[6] = d as f32;
    n[7] = g;
    n
}
