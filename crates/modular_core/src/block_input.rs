//! Whole-block input reads for modules declared with `#[module(block)]`.
//!
//! A block module has a per-sample `update`, which runs inside feedback cycles
//! (Sample mode), and a `process_block(outputs, range, sample_rate)`, which
//! runs everywhere else (Block mode). Both share the module's per-sample math.
//! `process_block` resolves every input for the whole `range` up front into a
//! [`BlockInput`], then runs plain loops over channel rows — no per-read
//! dispatch between constants and cables, no channel cycling, and no copy
//! through an intermediate output struct. [`map`] covers the common
//! elementwise case.
//!
//! An input that cannot be read in place is copied into the caller's
//! [`InputScratch`]: a mono cable broadcast across wider consumer channels
//! straight from the producer's buffer, and anything else — a cable from a
//! feedback cycle, a narrower poly cable cycled across wider channels,
//! permuted channels, a mix of cables and constants — one slot at a time,
//! exactly as a per-sample read would see it. When the range is too long for
//! the scratch, resolution returns `None`, and `process_block` returns `None`
//! too; it must therefore resolve every input before writing anything. The
//! generated wrapper then retries over sub-ranges of at most
//! [`SCRATCH_LEN`]` / channels` slots, and runs `update` for any sub-range
//! still declined — the route for a module whose input it cannot read by
//! block at all. Each call starts with the consumer's per-block index at
//! `range.start`.

use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::Range;

use crate::BlockPort;
use crate::dsp::utils::sanitize;
use crate::poly::{PORT_MAX_CHANNELS, PolySignal};
use crate::types::Signal;

/// Values one [`InputScratch`] holds: a whole 64-slot block of up to four
/// channels, and at least `PORT_MAX_CHANNELS`, so one slot of any module fits.
pub const SCRATCH_LEN: usize = 4 * PORT_MAX_CHANNELS;

/// Stack storage for an input that cannot be read in place: a constant row
/// broadcast across channels, or values copied out slot by slot.
pub struct InputScratch(MaybeUninit<[f32; SCRATCH_LEN]>);

impl InputScratch {
    /// Uninitialised; a [`BlockInput`] only reads the values it wrote.
    #[inline(always)]
    pub fn new() -> Self {
        Self(MaybeUninit::uninit())
    }

    fn as_mut_ptr(&mut self) -> *mut f32 {
        self.0.as_mut_ptr() as *mut f32
    }
}

impl Default for InputScratch {
    fn default() -> Self {
        Self::new()
    }
}

/// One input resolved for a range of slots: [`row(i)`](Self::row) is slot
/// `i`'s values across the consumer's channels.
#[derive(Clone, Copy)]
pub struct BlockInput<'a> {
    /// The first slot's row; slot `i`'s starts at `data + (i - first) * stride`.
    /// Stride 0 repeats one row.
    data: *const f32,
    first: usize,
    stride: usize,
    channels: usize,
    _rows: PhantomData<&'a [f32]>,
}

impl BlockInput<'_> {
    /// Slot `i`'s values. `i` must lie in the range the input was resolved for.
    #[inline(always)]
    pub fn row(&self, i: usize) -> &[f32] {
        // SAFETY: resolution guarantees `channels` initialised values per row
        // for every slot in the resolved range: a rendered producer buffer
        // (which outlives the call, see `PolyCableView`), the input's own
        // constants, or the borrowed scratch.
        unsafe {
            std::slice::from_raw_parts(self.data.add((i - self.first) * self.stride), self.channels)
        }
    }
}

impl PolySignal {
    /// Resolve this input for slots `range` across `channels` consumer
    /// channels, using `scratch` when it cannot be read in place. See the
    /// module docs for when this returns `None`.
    #[inline(always)]
    pub fn block_input<'a>(
        &'a self,
        range: Range<usize>,
        channels: usize,
        scratch: &'a mut InputScratch,
    ) -> Option<BlockInput<'a>> {
        // Inputs read in place resolve inline; copies go out of line.
        if channels <= self.channels() {
            let volts = self.volts();
            if !volts.is_empty() {
                return Some(BlockInput {
                    data: volts.as_ptr(),
                    first: range.start,
                    stride: 0,
                    channels,
                    _rows: PhantomData,
                });
            }
            if let Some((data, stride)) = self
                .cable_view()
                .and_then(|cable| cable.rendered_to(range.end))
            {
                return Some(BlockInput {
                    // SAFETY: `range.start < range.end`, a rendered slot.
                    data: unsafe { data.add(range.start * stride) },
                    first: range.start,
                    stride,
                    channels,
                    _rows: PhantomData,
                });
            }
        }
        // One slot reads each channel at the consumer's index, which the
        // wrapper leaves at `range.start`.
        if range.len() == 1 && self.volts().is_empty() {
            let row = scratch.as_mut_ptr();
            for ch in 0..channels {
                // SAFETY: `channels <= PORT_MAX_CHANNELS <= SCRATCH_LEN`.
                unsafe { row.add(ch).write(self.get_value(ch)) };
            }
            return Some(BlockInput {
                data: row,
                first: range.start,
                stride: 0,
                channels,
                _rows: PhantomData,
            });
        }
        self.copied_block_input(range, channels, scratch)
    }

    /// [`block_input`](Self::block_input) for inputs copied into `scratch`.
    #[inline(never)]
    fn copied_block_input<'a>(
        &'a self,
        range: Range<usize>,
        channels: usize,
        scratch: &'a mut InputScratch,
    ) -> Option<BlockInput<'a>> {
        let width = self.channels();
        let volts = self.volts();
        if !volts.is_empty() {
            let row = scratch.as_mut_ptr();
            for ch in 0..channels {
                // SAFETY: `channels <= PORT_MAX_CHANNELS <= SCRATCH_LEN`.
                unsafe { row.add(ch).write(volts[ch % width]) };
            }
            return Some(BlockInput {
                data: row,
                first: range.start,
                stride: 0,
                channels,
                _rows: PhantomData,
            });
        }
        if range.len() * channels > SCRATCH_LEN {
            return None;
        }
        if width == 1
            && let Some((data, stride)) = self
                .cable_view()
                .and_then(|cable| cable.rendered_to(range.end))
        {
            let rows = scratch.as_mut_ptr();
            for (n, i) in range.clone().enumerate() {
                // SAFETY: slot `i` is rendered; `range.len() * channels <=
                // SCRATCH_LEN`.
                unsafe {
                    let value = *data.add(i * stride);
                    for ch in 0..channels {
                        rows.add(n * channels + ch).write(value);
                    }
                }
            }
            return Some(BlockInput {
                data: rows,
                first: range.start,
                stride: channels,
                channels,
                _rows: PhantomData,
            });
        }
        // Each cable reads the slot its consumer's index names, so step that
        // index through the range. The wrapper resets it once `process_block`
        // returns.
        let index = self.signals().iter().find_map(|signal| match signal {
            Signal::Cable { index_ptr, .. } if !index_ptr.is_null() => Some(*index_ptr),
            _ => None,
        });
        let rows = scratch.as_mut_ptr();
        for (n, i) in range.clone().enumerate() {
            if let Some(index) = index {
                // SAFETY: the consumer wrapper's index cell, which outlives
                // its params; audio thread only.
                unsafe { (*index).set(i) };
            }
            for ch in 0..channels {
                // SAFETY: `range.len() * channels <= SCRATCH_LEN`.
                unsafe { rows.add(n * channels + ch).write(self.get_value(ch)) };
            }
        }
        Some(BlockInput {
            data: rows,
            first: range.start,
            stride: channels,
            channels,
            _rows: PhantomData,
        })
    }
}

/// A disconnected input's row when it defaults to zero, read in place.
static ZERO_ROW: [f32; PORT_MAX_CHANNELS] = [0.0; PORT_MAX_CHANNELS];

/// [`PolySignal::block_input`] for an optional input, broadcasting `default`
/// when it is disconnected.
pub trait OptionBlockInput {
    fn block_input_or<'a>(
        &'a self,
        range: Range<usize>,
        channels: usize,
        default: f32,
        scratch: &'a mut InputScratch,
    ) -> Option<BlockInput<'a>>;
}

impl OptionBlockInput for Option<PolySignal> {
    #[inline]
    fn block_input_or<'a>(
        &'a self,
        range: Range<usize>,
        channels: usize,
        default: f32,
        scratch: &'a mut InputScratch,
    ) -> Option<BlockInput<'a>> {
        match self {
            Some(signal) => signal.block_input(range, channels, scratch),
            None if default == 0.0 => Some(BlockInput {
                data: ZERO_ROW.as_ptr(),
                first: range.start,
                stride: 0,
                channels,
                _rows: PhantomData,
            }),
            None => {
                let row = scratch.as_mut_ptr();
                for ch in 0..channels {
                    // SAFETY: `channels <= PORT_MAX_CHANNELS <= SCRATCH_LEN`.
                    unsafe { row.add(ch).write(default) };
                }
                Some(BlockInput {
                    data: row,
                    first: range.start,
                    stride: 0,
                    channels,
                    _rows: PhantomData,
                })
            }
        }
    }
}

/// Elements per pass of [`map`]'s flat loop. Also bounds the channel count a
/// repeated row can be expanded across, so it must be at least
/// `PORT_MAX_CHANNELS`.
const CHUNK: usize = PORT_MAX_CHANNELS;

/// Write `f(inputs)` to every channel of `out` for slots `range`, sanitising
/// each result. The inputs must be resolved for `range` at `out`'s channel
/// count.
///
/// When every input is either contiguous rows or one repeated row, a
/// multi-slot range is processed as a single flat run, which vectorises at any
/// channel count; otherwise row by row.
#[inline(always)]
pub fn map<const N: usize>(
    out: &mut BlockPort,
    range: Range<usize>,
    inputs: [BlockInput<'_>; N],
    f: impl Fn([f32; N]) -> f32,
) {
    let channels = out.channels();
    debug_assert!(inputs.iter().all(|input| input.channels == channels));
    if channels == 0 {
        return;
    }
    let flat = range.len() > 1
        && inputs
            .iter()
            .all(|input| input.stride == channels || input.stride == 0);
    if !flat {
        for i in range {
            let rows: [&[f32]; N] = std::array::from_fn(|j| inputs[j].row(i));
            for (ch, value) in out.slot_mut(i).iter_mut().enumerate() {
                *value = sanitize(f(rows.map(|row| row[ch])));
            }
        }
        return;
    }
    // A repeated row is expanded into a whole number of rows filling (nearly)
    // a chunk, so every chunk starts on a row boundary and reads it from 0.
    let chunk = (CHUNK / channels * channels).min(range.len() * channels);
    let mut repeated = [const { MaybeUninit::<[f32; CHUNK]>::uninit() }; N];
    for (buffer, input) in repeated.iter_mut().zip(&inputs) {
        if input.stride == 0 {
            // SAFETY: `chunk <= CHUNK`; only these elements are written and,
            // below, read.
            let buffer =
                unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut f32, chunk) };
            let row = input.row(input.first);
            if let [value] = row {
                buffer.fill(*value);
            } else {
                for rows in buffer.chunks_exact_mut(channels) {
                    for (value, &v) in rows.iter_mut().zip(row) {
                        *value = v;
                    }
                }
            }
        }
    }
    for (n, out_chunk) in out.slots_mut(range).chunks_mut(chunk).enumerate() {
        let len = out_chunk.len();
        let offset = n * chunk;
        let sources: [&[f32]; N] = std::array::from_fn(|j| {
            let input = &inputs[j];
            if input.stride == 0 {
                // SAFETY: the first `chunk >= len` elements were written above.
                unsafe { std::slice::from_raw_parts(repeated[j].as_ptr() as *const f32, len) }
            } else {
                // SAFETY: contiguous rows from the range's first slot;
                // `offset + len` stays within the range's rows.
                unsafe { std::slice::from_raw_parts(input.data.add(offset), len) }
            }
        });
        for (k, value) in out_chunk.iter_mut().enumerate() {
            // SAFETY: every source has length `len` and `k < len`.
            *value = sanitize(f(sources.map(|source| unsafe { *source.get_unchecked(k) })));
        }
    }
}
