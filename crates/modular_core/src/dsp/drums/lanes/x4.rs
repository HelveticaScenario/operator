//! Four channels at once.

use std::ops::{
    Add, AddAssign, BitAnd, BitOr, Div, DivAssign, Mul, MulAssign, Neg, Not, Sub, SubAssign,
};

use wide::{f32x4, f64x4};

use super::vector::{broadcast_f32x4, broadcast_f64x4, switch_lanes, vector};
use super::{Lanes, Lanewise, Mask, Real};

/// Four channels.
#[derive(Clone, Copy, Default)]
pub struct X4;

#[derive(Clone, Copy, Default)]
pub struct F32x4(f32x4);

#[derive(Clone, Copy, Default)]
pub struct F64x4(f64x4);

/// Lane flags as all-ones or all-zeros lanes of their values.
#[derive(Clone, Copy, Default)]
pub struct M32x4(f32x4);

#[derive(Clone, Copy, Default)]
pub struct M64x4(f64x4);

vector!(F32x4, M32x4, f32x4, f32, broadcast_f32x4, 4);
vector!(F64x4, M64x4, f64x4, f64, broadcast_f64x4, 4);

impl Lanes for X4 {
    type S = F32x4;
    type D = F64x4;
    type SM = M32x4;
    type DM = M64x4;
    type Closed = [u8; 4];
    const WIDTH: usize = 4;

    #[inline(always)]
    fn wide(s: F32x4) -> F64x4 {
        F64x4(f64x4::new(s.0.to_array().map(|x| x as f64)))
    }
    #[inline(always)]
    fn narrow(d: F64x4) -> F32x4 {
        F32x4(f32x4::new(d.0.to_array().map(|x| x as f32)))
    }
    #[inline(always)]
    fn wide_mask(m: M32x4) -> M64x4 {
        M64x4::from_bits(m.bits())
    }
    #[inline(always)]
    fn narrow_mask(m: M64x4) -> M32x4 {
        M32x4::from_bits(m.bits())
    }
    switch_lanes!(M32x4, 4);

    #[inline(always)]
    fn gather(f: impl Fn(usize) -> f32) -> F32x4 {
        F32x4(f32x4::new(std::array::from_fn(f)))
    }
    #[inline(always)]
    fn mask(bits: u32) -> M32x4 {
        M32x4::from_bits(bits)
    }
}
