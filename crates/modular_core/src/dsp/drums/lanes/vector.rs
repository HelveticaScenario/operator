//! Vectors of lanes: `wide`'s vectors, with constants broadcast in a
//! register. `wide`'s own `splat` builds a repeat array, which LLVM can
//! lower to a `memset_pattern16` call per use on Apple targets. A vector
//! may hold more lanes than its group has channels; the spare lanes take
//! no part in its masks.

#[inline(always)]
pub(super) fn broadcast_f32x4(value: f32) -> wide::f32x4 {
    // SAFETY: both types are four `f32` lanes, and every bit pattern is a
    // valid value of each.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::mem::transmute(std::arch::aarch64::vdupq_n_f32(value))
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::mem::transmute(std::arch::x86_64::_mm_set1_ps(value))
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    wide::f32x4::splat(value)
}

#[inline(always)]
pub(super) fn broadcast_f64x2(value: f64) -> wide::f64x2 {
    // SAFETY: both types are two `f64` lanes, and every bit pattern is a
    // valid value of each.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::mem::transmute(std::arch::aarch64::vdupq_n_f64(value))
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::mem::transmute(std::arch::x86_64::_mm_set1_pd(value))
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    wide::f64x2::splat(value)
}

#[inline(always)]
pub(super) fn broadcast_f64x4(value: f64) -> wide::f64x4 {
    let half = broadcast_f64x2(value);
    // SAFETY: `f64x4` is two `f64x2` halves, whether or not it is one AVX
    // register, and every bit pattern is a valid value.
    unsafe { std::mem::transmute([half, half]) }
}

/// Implements the value and mask traits for a vector type `$v` over
/// `wide::$wide`, with mask `$m`, of which the first `$lanes` lanes are
/// the group's.
macro_rules! vector {
    ($v:ident, $m:ident, $wide:ident, $elem:ty, $broadcast:ident, $lanes:expr) => {
        impl $m {
            /// Lane `l` set where bit `l` of `bits` is.
            #[inline(always)]
            pub fn from_bits(bits: u32) -> Self {
                Self($wide::new(std::array::from_fn(|lane| {
                    <$elem>::from_bits(if bits & (1 << lane) != 0 { !0 } else { 0 })
                })))
            }
        }

        impl Add for $v {
            type Output = Self;
            #[inline(always)]
            fn add(self, rhs: Self) -> Self {
                Self(self.0 + rhs.0)
            }
        }
        impl Sub for $v {
            type Output = Self;
            #[inline(always)]
            fn sub(self, rhs: Self) -> Self {
                Self(self.0 - rhs.0)
            }
        }
        impl Mul for $v {
            type Output = Self;
            #[inline(always)]
            fn mul(self, rhs: Self) -> Self {
                Self(self.0 * rhs.0)
            }
        }
        impl Div for $v {
            type Output = Self;
            #[inline(always)]
            fn div(self, rhs: Self) -> Self {
                Self(self.0 / rhs.0)
            }
        }
        impl Neg for $v {
            type Output = Self;
            #[inline(always)]
            fn neg(self) -> Self {
                Self(-self.0)
            }
        }
        impl Add<$elem> for $v {
            type Output = Self;
            #[inline(always)]
            fn add(self, rhs: $elem) -> Self {
                Self(self.0 + $broadcast(rhs))
            }
        }
        impl Sub<$elem> for $v {
            type Output = Self;
            #[inline(always)]
            fn sub(self, rhs: $elem) -> Self {
                Self(self.0 - $broadcast(rhs))
            }
        }
        impl Mul<$elem> for $v {
            type Output = Self;
            #[inline(always)]
            fn mul(self, rhs: $elem) -> Self {
                Self(self.0 * $broadcast(rhs))
            }
        }
        impl Div<$elem> for $v {
            type Output = Self;
            #[inline(always)]
            fn div(self, rhs: $elem) -> Self {
                Self(self.0 / $broadcast(rhs))
            }
        }
        impl AddAssign for $v {
            #[inline(always)]
            fn add_assign(&mut self, rhs: Self) {
                *self = *self + rhs;
            }
        }
        impl SubAssign for $v {
            #[inline(always)]
            fn sub_assign(&mut self, rhs: Self) {
                *self = *self - rhs;
            }
        }
        impl MulAssign for $v {
            #[inline(always)]
            fn mul_assign(&mut self, rhs: Self) {
                *self = *self * rhs;
            }
        }
        impl DivAssign for $v {
            #[inline(always)]
            fn div_assign(&mut self, rhs: Self) {
                *self = *self / rhs;
            }
        }
        impl AddAssign<$elem> for $v {
            #[inline(always)]
            fn add_assign(&mut self, rhs: $elem) {
                *self = *self + rhs;
            }
        }
        impl SubAssign<$elem> for $v {
            #[inline(always)]
            fn sub_assign(&mut self, rhs: $elem) {
                *self = *self - rhs;
            }
        }
        impl MulAssign<$elem> for $v {
            #[inline(always)]
            fn mul_assign(&mut self, rhs: $elem) {
                *self = *self * rhs;
            }
        }
        impl DivAssign<$elem> for $v {
            #[inline(always)]
            fn div_assign(&mut self, rhs: $elem) {
                *self = *self / rhs;
            }
        }

        impl BitAnd for $m {
            type Output = Self;
            #[inline(always)]
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }
        impl BitOr for $m {
            type Output = Self;
            #[inline(always)]
            fn bitor(self, rhs: Self) -> Self {
                Self(self.0 | rhs.0)
            }
        }
        impl Not for $m {
            type Output = Self;
            #[inline(always)]
            fn not(self) -> Self {
                Self(!self.0)
            }
        }
        impl Mask for $m {
            #[inline(always)]
            fn any(self) -> bool {
                self.bits() != 0
            }
            #[inline(always)]
            fn all(self) -> bool {
                self.bits() == (1 << $lanes) - 1
            }
            #[inline(always)]
            fn bits(self) -> u32 {
                self.0.to_bitmask() & ((1 << $lanes) - 1)
            }
        }
        impl Lanewise for $m {
            type Lane = bool;
            #[inline]
            fn lane(&self, lane: usize) -> bool {
                self.bits() & (1 << lane) != 0
            }
            #[inline]
            fn set_lane(&mut self, lane: usize, value: &bool) {
                let bits = self.bits() & !(1 << lane) | ((*value as u32) << lane);
                *self = $m::from_bits(bits);
            }
        }

        impl Lanewise for $v {
            type Lane = $elem;
            #[inline]
            fn lane(&self, lane: usize) -> $elem {
                self.0.to_array()[lane]
            }
            #[inline]
            fn set_lane(&mut self, lane: usize, value: &$elem) {
                let mut lanes = self.0.to_array();
                lanes[lane] = *value;
                self.0 = $wide::new(lanes);
            }
        }

        impl Real for $v {
            type Elem = $elem;
            type Mask = $m;

            #[inline(always)]
            fn splat(value: $elem) -> Self {
                Self($broadcast(value))
            }
            #[inline(always)]
            fn mul_add(self, a: Self, b: Self) -> Self {
                Self(self.0.mul_add(a.0, b.0))
            }
            #[inline(always)]
            fn min(self, rhs: Self) -> Self {
                Self(self.0.min(rhs.0))
            }
            #[inline(always)]
            fn max(self, rhs: Self) -> Self {
                Self(self.0.max(rhs.0))
            }
            #[inline(always)]
            fn clamp(self, lo: Self, hi: Self) -> Self {
                let x = Self::select(self.lt(lo), lo, self);
                Self::select(x.gt(hi), hi, x)
            }
            #[inline(always)]
            fn abs(self) -> Self {
                Self(self.0.abs())
            }
            #[inline(always)]
            fn floor(self) -> Self {
                Self(self.0.floor())
            }
            #[inline(always)]
            fn sanitize(self) -> Self {
                Self::select(
                    self.abs().lt(Self::splat(<$elem>::INFINITY)),
                    self,
                    Self::splat(0.0),
                )
            }
            #[inline(always)]
            fn fract(self) -> Self {
                Self(self.0 - self.0.trunc())
            }
            #[inline(always)]
            fn lt(self, rhs: Self) -> $m {
                $m(self.0.simd_lt(rhs.0))
            }
            #[inline(always)]
            fn gt(self, rhs: Self) -> $m {
                $m(self.0.simd_gt(rhs.0))
            }
            #[inline(always)]
            fn ge(self, rhs: Self) -> $m {
                $m(self.0.simd_ge(rhs.0))
            }
            #[inline(always)]
            fn select(mask: $m, if_true: Self, if_false: Self) -> Self {
                Self(mask.0.select(if_true.0, if_false.0))
            }
            #[inline(always)]
            fn map(self, f: impl Fn($elem) -> $elem) -> Self {
                let mut lanes = self.0.to_array();
                for x in lanes.iter_mut().take($lanes) {
                    *x = f(*x);
                }
                Self($wide::new(lanes))
            }
            #[inline(always)]
            fn map_where(self, mask: $m, keep: Self, f: impl Fn($elem) -> $elem) -> Self {
                let bits = mask.bits();
                let (x, mut out) = (self.0.to_array(), keep.0.to_array());
                for lane in 0..$lanes {
                    if bits & (1 << lane) != 0 {
                        out[lane] = f(x[lane]);
                    }
                }
                Self($wide::new(out))
            }
        }
    };
}

pub(super) use vector;

/// The [`Lanes`](super::Lanes) items for a group's switch combinations,
/// one per lane in `[u8; $lanes]`, with switch masks `$m`.
macro_rules! switch_lanes {
    ($m:ident, $lanes:expr) => {
        #[inline(always)]
        fn closed<const S: usize>(closed: [$m; S]) -> [u8; $lanes] {
            let bits = closed.map(|m| m.bits());
            std::array::from_fn(|lane| {
                bits.iter()
                    .enumerate()
                    .fold(0, |k, (s, b)| k | ((((b >> lane) & 1) as u8) << s))
            })
        }
        #[inline(always)]
        fn uniform(closed: [u8; $lanes]) -> Option<usize> {
            let k = closed[0];
            closed.iter().all(|&c| c == k).then_some(k as usize)
        }
        #[inline(always)]
        fn closed_lane(closed: [u8; $lanes], lane: usize) -> usize {
            closed[lane] as usize
        }
        #[inline(always)]
        fn switch_on(closed: [u8; $lanes], s: usize) -> $m {
            $m::from_bits(closed.iter().enumerate().fold(0, |bits, (lane, &k)| {
                bits | ((((k >> s) & 1) as u32) << lane)
            }))
        }
    };
}
pub(super) use switch_lanes;
