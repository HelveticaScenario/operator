//! The grain recording buffer: a mono delay line stored at the bit depth of
//! the selected quality, with Beads' freeze-tail crossfade.

use std::sync::Arc;

use super::primitives::{clip16, smooth_step};

/// Storage format of the recording buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// 8-bit µ-law.
    MuLaw8,
    /// 12-bit linear.
    Linear12,
    /// 16-bit linear.
    Linear16,
}

/// Extra samples mirrored past the end so 4-point reads never wrap.
const INTERPOLATION_TAIL: usize = 4;
/// Samples captured while frozen and crossfaded back in when recording resumes.
const FREEZE_TAIL_SIZE: usize = 256;

/// Largest loaded buffer, so read indices up to three buffer lengths fit an
/// `i32`.
pub const MAX_LOADED_FRAMES: usize = 1 << 29;

/// Magnitudes of the µ-law decode table for codes 128–255, rounded to five
/// decimals as in the hardware; codes 0–127 decode to their negatives.
#[rustfmt::skip]
#[allow(clippy::approx_constant)]
const MULAW_MAGNITUDES: [f32; 128] = [
    0.98035, 0.94910, 0.91785, 0.88660, 0.85535, 0.82410, 0.79285, 0.76160,
    0.73035, 0.69910, 0.66785, 0.63660, 0.60535, 0.57410, 0.54285, 0.51160,
    0.48816, 0.47253, 0.45691, 0.44128, 0.42566, 0.41003, 0.39441, 0.37878,
    0.36316, 0.34753, 0.33191, 0.31628, 0.30066, 0.28503, 0.26941, 0.25378,
    0.24207, 0.23425, 0.22644, 0.21863, 0.21082, 0.20300, 0.19519, 0.18738,
    0.17957, 0.17175, 0.16394, 0.15613, 0.14832, 0.14050, 0.13269, 0.12488,
    0.11902, 0.11511, 0.11121, 0.10730, 0.10339, 0.09949, 0.09558, 0.09167,
    0.08777, 0.08386, 0.07996, 0.07605, 0.07214, 0.06824, 0.06433, 0.06042,
    0.05750, 0.05554, 0.05359, 0.05164, 0.04968, 0.04773, 0.04578, 0.04382,
    0.04187, 0.03992, 0.03796, 0.03601, 0.03406, 0.03210, 0.03015, 0.02820,
    0.02673, 0.02576, 0.02478, 0.02380, 0.02283, 0.02185, 0.02087, 0.01990,
    0.01892, 0.01794, 0.01697, 0.01599, 0.01501, 0.01404, 0.01306, 0.01208,
    0.01135, 0.01086, 0.01038, 0.00989, 0.00940, 0.00891, 0.00842, 0.00793,
    0.00745, 0.00696, 0.00647, 0.00598, 0.00549, 0.00500, 0.00452, 0.00403,
    0.00366, 0.00342, 0.00317, 0.00293, 0.00269, 0.00244, 0.00220, 0.00195,
    0.00171, 0.00146, 0.00122, 0.00098, 0.00073, 0.00049, 0.00024, 0.00000,
];

#[inline]
pub fn mulaw_to_linear(code: u8) -> f32 {
    let magnitude = MULAW_MAGNITUDES[(code & 0x7f) as usize];
    if code & 0x80 != 0 {
        magnitude
    } else {
        -magnitude
    }
}

#[inline]
pub fn linear_to_mulaw(pcm: i32) -> u8 {
    let mut pcm = pcm >> 2;
    let mask: u8 = if pcm < 0 {
        pcm = -pcm;
        0x7f
    } else {
        0xff
    };
    pcm = pcm.min(8159) + (0x84 >> 2);
    let seg = [0x3f, 0x7f, 0xff, 0x1ff, 0x3ff, 0x7ff, 0xfff, 0x1fff]
        .iter()
        .position(|&limit| pcm <= limit)
        .unwrap_or(8) as u8;
    if seg >= 8 {
        0x7f ^ mask
    } else {
        ((seg << 4) | ((pcm >> (seg + 1)) as u8 & 0x0f)) ^ mask
    }
}

/// µ-law round trip on a full-scale sample, as applied to the dry signal in
/// dirty-tape mode.
#[inline]
pub fn mulaw_round_trip(x: f32) -> f32 {
    mulaw_to_linear(linear_to_mulaw(clip16((x * 32768.0) as i32)))
}

/// A sample in the storage format of `resolution`. µ-law codes are stored
/// inverted so that silence is stored as 0 in every format, and zeroed
/// storage needs no clearing.
#[inline]
fn encode(resolution: Resolution, x: f32) -> i16 {
    let pcm = clip16((x * 32768.0) as i32);
    match resolution {
        Resolution::MuLaw8 => (linear_to_mulaw(pcm) ^ 0xff) as i16,
        Resolution::Linear12 => (pcm >> 4) as i16,
        Resolution::Linear16 => pcm as i16,
    }
}

/// Decoder for one storage format, so grain reads compile without a
/// per-sample format branch.
pub trait Codec {
    fn decode(v: i16) -> f32;
}

pub struct MuLaw8Codec;
pub struct Linear12Codec;
pub struct Linear16Codec;

impl Codec for MuLaw8Codec {
    #[inline(always)]
    fn decode(v: i16) -> f32 {
        mulaw_to_linear(v as u8 ^ 0xff)
    }
}

impl Codec for Linear12Codec {
    #[inline(always)]
    fn decode(v: i16) -> f32 {
        v as f32 * (1.0 / 2048.0)
    }
}

impl Codec for Linear16Codec {
    #[inline(always)]
    fn decode(v: i16) -> f32 {
        v as f32 * (1.0 / 32768.0)
    }
}

/// A buffer's samples: recorded in place, or loaded and shared read-only.
#[derive(Clone)]
enum Items {
    Recorded(Vec<i16>),
    Loaded(Arc<[i16]>),
}

#[derive(Clone)]
pub struct GrainBuffer {
    /// Samples covering the buffer length plus the interpolation tail.
    items: Items,
    freeze_tail: [f32; FREEZE_TAIL_SIZE],
    resolution: Resolution,
    max_delay: i32,
    write_ptr: i32,
    tail_write_ptr: usize,
    tail_xfade_duration: usize,
    written_since_last_frozen: i32,
    duration: i32,
    frozen: bool,
    /// Identity of a loaded buffer's contents; 0 for a recording.
    source: u64,
    /// Whether every stored sample is still 0, as allocated.
    clean: bool,
}

impl Default for GrainBuffer {
    fn default() -> Self {
        Self {
            items: Items::Recorded(Vec::new()),
            freeze_tail: [0.0; FREEZE_TAIL_SIZE],
            resolution: Resolution::Linear16,
            max_delay: 1,
            write_ptr: 0,
            tail_write_ptr: 0,
            tail_xfade_duration: 0,
            written_since_last_frozen: 0,
            duration: 1,
            frozen: false,
            source: 0,
            clean: true,
        }
    }
}

impl GrainBuffer {
    /// Allocate silent storage for `frames` samples. Zeroed allocations
    /// come from the system already cleared, so this is cheap even for long
    /// buffers. Main thread only.
    pub fn allocate_frames(&mut self, frames: usize) {
        self.items = Items::Recorded(vec![0; frames + INTERPOLATION_TAIL]);
        self.clean = true;
    }

    /// Samples of storage, without the interpolation tail.
    pub fn frames(&self) -> usize {
        self.items().len().saturating_sub(INTERPOLATION_TAIL)
    }

    /// Whether the buffer has storage to record into.
    pub fn records(&self) -> bool {
        matches!(&self.items, Items::Recorded(v) if !v.is_empty())
    }

    /// Make the contents read-only and cheap to clone; clones share them.
    /// The buffer stays frozen from then on. Main thread only.
    pub fn share(&mut self) {
        if let Items::Recorded(v) = &mut self.items {
            self.items = Items::Loaded(std::mem::take(v).into());
        }
    }

    /// The stored sample at `i`.
    #[cfg(test)]
    pub fn raw(&self, i: usize) -> i16 {
        self.items()[i]
    }

    /// Whether another buffer reads this one's shared contents.
    pub fn shared_elsewhere(&self) -> bool {
        match &self.items {
            Items::Recorded(_) => false,
            Items::Loaded(a) => Arc::strong_count(a) > 1,
        }
    }

    /// Whether both buffers read the same shared contents.
    #[cfg(test)]
    pub fn shares_with(&self, other: &GrainBuffer) -> bool {
        match (&self.items, &other.items) {
            (Items::Loaded(a), Items::Loaded(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    #[inline(always)]
    fn items(&self) -> &[i16] {
        match &self.items {
            Items::Recorded(v) => v,
            Items::Loaded(a) => a,
        }
    }

    /// Switch format and length, clearing recorded contents; a shared
    /// buffer keeps its own. Allocation-free. The length is capped to the
    /// storage, and an unallocated buffer must not be read.
    pub fn reset(&mut self, resolution: Resolution, max_delay: usize) {
        let max_delay = max_delay
            .min(self.items().len().saturating_sub(INTERPOLATION_TAIL))
            .max(1);
        self.source = 0;
        self.resolution = resolution;
        self.max_delay = max_delay as i32;
        self.write_ptr = 0;
        self.tail_write_ptr = 0;
        self.tail_xfade_duration = 0;
        self.frozen = false;
        self.written_since_last_frozen = 0;
        self.duration = self.max_delay;
        if let Items::Recorded(v) = &mut self.items
            && !self.clean
        {
            let used = (max_delay + INTERPOLATION_TAIL).min(v.len());
            v[..used].fill(0);
        }
        // Recording starts as soon as the buffer is reset.
        self.clean = false;
        self.freeze_tail = [0.0; FREEZE_TAIL_SIZE];
    }

    pub fn source(&self) -> u64 {
        self.source
    }

    pub fn set_source(&mut self, source: u64) {
        self.source = source;
    }

    #[inline]
    pub fn max_delay(&self) -> i32 {
        self.max_delay
    }

    #[inline]
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// Recorded length in samples, up to [`Self::max_delay`].
    #[inline]
    pub fn duration(&self) -> i32 {
        self.duration
    }

    #[inline]
    fn store(&mut self, i: usize, x: f32) {
        if let Items::Recorded(v) = &mut self.items {
            v[i] = encode(self.resolution, x);
        }
    }

    fn write_unfrozen(&mut self, x: f32) {
        let w = self.write_ptr as usize;
        self.store(w, x);
        if w < INTERPOLATION_TAIL {
            self.store(w + self.max_delay as usize, x);
        }
        self.write_ptr = if self.write_ptr == 0 {
            self.max_delay - 1
        } else {
            self.write_ptr - 1
        };
        self.written_since_last_frozen += 1;
        if self.written_since_last_frozen >= self.max_delay {
            self.written_since_last_frozen = 0;
        }
        self.duration = (self.duration + 1).min(self.max_delay);
    }

    /// Record one sample. While `frozen`, recording stops and the incoming
    /// audio is kept in a short tail that is crossfaded in on release.
    pub fn write(&mut self, mut x: f32, frozen: bool) {
        let frozen = frozen || matches!(self.items, Items::Loaded(_));
        if frozen {
            if !self.frozen {
                self.tail_write_ptr = 0;
            }
            if self.tail_write_ptr < FREEZE_TAIL_SIZE {
                self.freeze_tail[self.tail_write_ptr] = x;
                self.tail_write_ptr += 1;
            }
            self.written_since_last_frozen = 0;
        } else {
            if self.frozen && self.tail_write_ptr > 0 {
                self.tail_xfade_duration = self.tail_write_ptr;
            }
            if self.tail_write_ptr > 0 {
                let position = self.tail_xfade_duration - self.tail_write_ptr;
                let t = self.tail_write_ptr as f32 / self.tail_xfade_duration as f32;
                let gain = smooth_step(t);
                x += (self.freeze_tail[position] - x) * gain;
                self.tail_write_ptr -= 1;
            }
            self.write_unfrozen(x);
        }
        self.frozen = frozen;
    }

    #[inline]
    pub fn resolution(&self) -> Resolution {
        self.resolution
    }

    /// 4-point Hermite read `delay_integral + delay_fractional` samples
    /// behind the write head; `delay_integral` is in `0..max_delay`. `C` must
    /// match [`Self::resolution`].
    #[inline(always)]
    pub fn read_hermite<C: Codec>(&self, delay_integral: i32, delay_fractional: f32) -> f32 {
        let max = self.max_delay;
        let mut i = self.write_ptr + delay_integral + max - 1;
        if i >= max {
            i -= max;
        }
        if i >= max {
            i -= max;
        }
        let i = i as usize;
        let s = &self.items()[i..i + 4];
        let x0 = C::decode(s[0]);
        let x1 = C::decode(s[1]);
        let x2 = C::decode(s[2]);
        let x3 = C::decode(s[3]);
        let c = (x2 - x0) * 0.5;
        let v = x1 - x2;
        let w = c + v;
        let a = w + v + (x3 - x1) * 0.5;
        let b_neg = w + a;
        let t = delay_fractional;
        (((a * t) - b_neg) * t + c) * t + x1
    }
}
