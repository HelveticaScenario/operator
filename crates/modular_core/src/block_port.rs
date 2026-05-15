//! Block-sized port buffer.
//!
//! Layout: `data[sample_index][channel_index]`
//!
//! All channel values at the same sample index are contiguous in memory,
//! enabling future SIMD optimization. Heap-allocated once at construction;
//! never resized on the audio thread.

use crate::poly::PORT_MAX_CHANNELS;

/// A pre-allocated buffer holding `block_size` samples, each with `PORT_MAX_CHANNELS` channels.
///
/// `data[i][ch]` is the value for sample index `i`, channel `ch`.
pub struct BlockPort {
    /// `data.len() == block_size` (set at construction, never changed).
    pub data: Box<[[f32; PORT_MAX_CHANNELS]]>,
}

impl BlockPort {
    /// Allocate a new zeroed port buffer for the given block size.
    ///
    /// **Must not be called on the audio thread** (allocates heap memory).
    pub fn new(block_size: usize) -> Self {
        Self {
            data: vec![[0.0f32; PORT_MAX_CHANNELS]; block_size].into_boxed_slice(),
        }
    }

    /// Read value at `(index, ch)`, returning `0.0` for out-of-range accesses.
    #[inline]
    pub fn get(&self, index: usize, ch: usize) -> f32 {
        self.data
            .get(index)
            .and_then(|slot| slot.get(ch).copied())
            .unwrap_or(0.0)
    }

    /// Write value at `(index, ch)`. Silently ignored if out of range.
    #[inline]
    pub fn set(&mut self, index: usize, ch: usize, value: f32) {
        if let Some(slot) = self.data.get_mut(index) {
            if let Some(cell) = slot.get_mut(ch) {
                *cell = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PORT_MAX_CHANNELS;

    #[test]
    fn block_port_new_zeroed() {
        let bp = BlockPort::new(4);
        assert_eq!(bp.data.len(), 4);
        for slot in bp.data.iter() {
            assert_eq!(*slot, [0.0f32; PORT_MAX_CHANNELS]);
        }
    }

    #[test]
    fn block_port_get_in_range() {
        let mut bp = BlockPort::new(4);
        bp.data[2][3] = 1.5;
        assert_eq!(bp.get(2, 3), 1.5);
    }

    #[test]
    fn block_port_get_out_of_range() {
        let bp = BlockPort::new(4);
        assert_eq!(bp.get(99, 0), 0.0);
        assert_eq!(bp.get(0, 99), 0.0);
    }

    #[test]
    fn block_port_set() {
        let mut bp = BlockPort::new(4);
        bp.set(1, 2, 3.14);
        assert!((bp.get(1, 2) - 3.14).abs() < 1e-6);
    }
}
