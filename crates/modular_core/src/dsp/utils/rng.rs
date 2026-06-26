//! Shared pseudo-random helpers for the audio thread.
//!
//! A small linear-congruential generator plus a stable, per-instance seeding
//! scheme, used by the modules that need cheap non-deterministic randomness:
//! noise color, dither, grain jitter, and oscillator phase spread.
//!
//! The pattern system deliberately does **not** use this — its randomness must
//! be deterministic and reproducible.

/// Odd multiple of the golden ratio, used to decorrelate per-stream seeds.
const GOLDEN_RATIO_ODD: u64 = 0x9E37_79B9_7F4A_7C15;

/// LCG multiplier (Knuth MMIX). Paired with an odd increment it has a full
/// 2^64 period.
const LCG_MULTIPLIER: u64 = 6364136223846793005;

/// Reciprocal of 2^32, mapping a `u32` into `[0, 1)`.
const U32_TO_UNIT: f32 = 1.0 / 4_294_967_296.0;

/// Stable per-instance seed base: the module's heap address.
///
/// Call this once the module sits at its final boxed heap address — from
/// `on_patch_update`, or from `init` only when a fresh per-stream seed every
/// (re)construction is acceptable. Two modules constructed back-to-back can
/// share a transient stack address during `init`, so RNGs that must stay
/// distinct between instances should seed from `on_patch_update`.
#[inline]
pub fn seed_base<T>(module: &T) -> u64 {
    module as *const T as usize as u64
}

/// Cheap, non-cryptographic linear-congruential generator for audio-thread
/// randomness (noise, dither, grain jitter, phase spread). One instance per
/// stream — per channel, or per voice.
#[derive(Default, Clone, Copy)]
pub struct LcgRng {
    state: u64,
}

impl LcgRng {
    /// Seed this stream from a [`seed_base`] and a stream index. Mixing the
    /// index with an odd golden-ratio constant decorrelates streams that share
    /// a base (e.g. every channel of one module).
    #[inline]
    pub fn seed(&mut self, base: u64, stream: usize) {
        self.state = base ^ (stream as u64).wrapping_mul(GOLDEN_RATIO_ODD);
    }

    /// Advance the generator and return the high 32 bits (the low bits of an LCG
    /// are weak, so the top half is used).
    #[inline]
    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(LCG_MULTIPLIER).wrapping_add(1);
        (self.state >> 32) as u32
    }

    /// Next pseudo-random value in `[0, 1)`.
    #[inline]
    pub fn next_unit(&mut self) -> f32 {
        self.next_u32() as f32 * U32_TO_UNIT
    }

    /// Next pseudo-random value in `[-1, 1)`.
    #[inline]
    pub fn next_bipolar(&mut self) -> f32 {
        self.next_unit() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outputs_stay_in_range() {
        let mut rng = LcgRng::default();
        rng.seed(0x1234_5678, 0);
        for _ in 0..100_000 {
            let u = rng.next_unit();
            assert!((0.0..1.0).contains(&u), "next_unit out of range: {u}");
            let b = rng.next_bipolar();
            assert!((-1.0..1.0).contains(&b), "next_bipolar out of range: {b}");
        }
    }

    #[test]
    fn seeding_is_deterministic() {
        let (mut a, mut b) = (LcgRng::default(), LcgRng::default());
        a.seed(0xDEAD_BEEF, 3);
        b.seed(0xDEAD_BEEF, 3);
        for _ in 0..1000 {
            assert_eq!(a.next_unit(), b.next_unit());
        }
    }

    #[test]
    fn distinct_streams_decorrelate() {
        // Same base, different stream index → different sequences.
        let base = 0xABCD_1234;
        let mut a = LcgRng::default();
        let mut b = LcgRng::default();
        a.seed(base, 0);
        b.seed(base, 1);
        let same = (0..16).all(|_| (a.next_unit() - b.next_unit()).abs() < f32::EPSILON);
        assert!(!same, "streams 0 and 1 produced identical sequences");
    }
}
