//! Block-sized port buffer.
//!
//! Layout: a flat `block_size * channels` slice, indexed `data[index * channels + ch]`.
//! All channel values at the same sample index are contiguous in memory,
//! enabling future SIMD optimization. Heap-allocated once at construction;
//! never resized on the audio thread.

/// A pre-allocated buffer holding `block_size` samples, each with `channels`
/// channels. The channel count is the producing port's width: 1 for a mono
/// (f32) output, the module's channel count for a polyphonic output.
///
/// Reads cycle (`ch % channels`) so a consumer with more channels than the
/// producer still sees the producer's value broadcast/wrapped across its
/// channels — preserving the mono→poly broadcast semantics.
pub struct BlockPort {
    /// Flat `block_size * channels` voltages. Length never changes after
    /// construction.
    data: Box<[f32]>,
    /// Per-port channel width (`>= 1` for live ports, `0` only when the
    /// module is zero-channel).
    channels: usize,
    /// Number of sample slots (the internal block size).
    block_size: usize,
}

impl BlockPort {
    /// Allocate a new zeroed port buffer for the given block size and channel
    /// width.
    ///
    /// **Must not be called on the audio thread** (allocates heap memory).
    pub fn new(block_size: usize, channels: usize) -> Self {
        Self {
            data: vec![0.0f32; block_size * channels].into_boxed_slice(),
            channels,
            block_size,
        }
    }

    /// This port's channel width.
    #[inline]
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Read value at `(index, ch)`, returning `0.0` for out-of-range sample
    /// indices or a zero-channel port. The channel index cycles modulo the
    /// port width, so a consumer reading a higher channel than the producer
    /// has wraps around — preserving mono-broadcast / poly-cycling semantics.
    #[inline]
    pub fn get(&self, index: usize, ch: usize) -> f32 {
        if self.channels == 0 || index >= self.block_size {
            return 0.0;
        }
        self.data[index * self.channels + ch % self.channels]
    }

    /// Write value at `(index, ch)`. Silently ignored if out of range. The
    /// channel index is raw (no cycling): callers write `0..channels`.
    #[inline]
    pub fn set(&mut self, index: usize, ch: usize, value: f32) {
        if index < self.block_size && ch < self.channels {
            self.data[index * self.channels + ch] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_port_new_zeroed() {
        let bp = BlockPort::new(4, 16);
        assert_eq!(bp.channels(), 16);
        for index in 0..4 {
            for ch in 0..16 {
                assert_eq!(bp.get(index, ch), 0.0);
            }
        }
    }

    #[test]
    fn block_port_get_in_range() {
        let mut bp = BlockPort::new(4, 16);
        bp.set(2, 3, 1.5);
        assert_eq!(bp.get(2, 3), 1.5);
    }

    #[test]
    fn block_port_get_out_of_range() {
        let bp = BlockPort::new(4, 16);
        // Sample index out of range → 0.0.
        assert_eq!(bp.get(99, 0), 0.0);
    }

    #[test]
    fn block_port_channel_cycles() {
        // A mono (width-1) port broadcasts to every consumer channel.
        let mut bp = BlockPort::new(4, 1);
        bp.set(1, 0, 2.5);
        assert_eq!(bp.get(1, 0), 2.5);
        assert_eq!(bp.get(1, 5), 2.5); // 5 % 1 == 0
        // A width-2 port wraps higher channels.
        let mut poly = BlockPort::new(4, 2);
        poly.set(0, 0, 1.0);
        poly.set(0, 1, 2.0);
        assert_eq!(poly.get(0, 2), 1.0); // 2 % 2 == 0
        assert_eq!(poly.get(0, 3), 2.0); // 3 % 2 == 1
    }

    #[test]
    fn block_port_set() {
        let mut bp = BlockPort::new(4, 16);
        bp.set(1, 2, 3.14);
        assert!((bp.get(1, 2) - 3.14).abs() < 1e-6);
    }
}
