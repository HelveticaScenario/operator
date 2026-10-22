//! Two channels at once: each double-precision value in one 128-bit
//! register, as the scalar code's are in one register each.

use std::ops::{
    Add, AddAssign, BitAnd, BitOr, Div, DivAssign, Mul, MulAssign, Neg, Not, Sub, SubAssign,
};

use wide::{f32x4, f64x2};

use super::vector::{broadcast_f32x4, broadcast_f64x2, switch_lanes, vector};
use super::{Lanes, Lanewise, Mask, Real};

/// Two channels.
#[derive(Clone, Copy, Default)]
pub struct X2;

/// Two `f32` lanes in the low half of a 128-bit vector.
#[derive(Clone, Copy, Default)]
pub struct F32x2(f32x4);

#[derive(Clone, Copy, Default)]
pub struct F64x2(f64x2);

/// Lane flags as all-ones or all-zeros lanes of their values.
#[derive(Clone, Copy, Default)]
pub struct M32x2(f32x4);

#[derive(Clone, Copy, Default)]
pub struct M64x2(f64x2);

vector!(F32x2, M32x2, f32x4, f32, broadcast_f32x4, 2);
vector!(F64x2, M64x2, f64x2, f64, broadcast_f64x2, 2);

impl Lanes for X2 {
    type S = F32x2;
    type D = F64x2;
    type SM = M32x2;
    type DM = M64x2;
    type Closed = [u8; 2];
    const WIDTH: usize = 2;

    #[inline(always)]
    fn wide(s: F32x2) -> F64x2 {
        let [a, b, ..] = s.0.to_array();
        F64x2(f64x2::new([a as f64, b as f64]))
    }
    #[inline(always)]
    fn narrow(d: F64x2) -> F32x2 {
        let [a, b] = d.0.to_array();
        F32x2(f32x4::new([a as f32, b as f32, 0.0, 0.0]))
    }
    #[inline(always)]
    fn wide_mask(m: M32x2) -> M64x2 {
        M64x2::from_bits(m.bits())
    }
    #[inline(always)]
    fn narrow_mask(m: M64x2) -> M32x2 {
        M32x2::from_bits(m.bits())
    }

    switch_lanes!(M32x2, 2);

    #[inline(always)]
    fn gather(f: impl Fn(usize) -> f32) -> F32x2 {
        F32x2(f32x4::new([f(0), f(1), 0.0, 0.0]))
    }
    #[inline(always)]
    fn mask(bits: u32) -> M32x2 {
        M32x2::from_bits(bits)
    }
}
