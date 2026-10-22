//! A drum's inputs for the sample it runs, read live by `update` and from
//! resolved block rows by `process_block`, so both run the same per-sample
//! code.

use std::ops::Range;

use crate::BlockPort;
use crate::block_input::BlockInput;
use crate::dsp::utils::sanitize;
use crate::poly::PolySignal;

/// One input's value on each channel for the sample being run.
pub trait Input: Copy {
    fn at(self, ch: usize) -> f32;
}

/// An input read at the module's current slot; `default` while
/// disconnected.
#[derive(Clone, Copy)]
pub struct Live<'a> {
    signal: Option<&'a PolySignal>,
    default: f32,
}

impl<'a> Live<'a> {
    pub fn new(signal: &'a PolySignal) -> Self {
        Self {
            signal: Some(signal),
            default: 0.0,
        }
    }

    pub fn or(signal: &'a Option<PolySignal>, default: f32) -> Self {
        Self {
            signal: signal.as_ref(),
            default,
        }
    }
}

impl Input for Live<'_> {
    #[inline(always)]
    fn at(self, ch: usize) -> f32 {
        match self.signal {
            Some(signal) => signal.get_value(ch),
            None => self.default,
        }
    }
}

/// One slot's row of a resolved block input.
impl Input for &[f32] {
    #[inline(always)]
    fn at(self, ch: usize) -> f32 {
        self[ch]
    }
}

/// A setting derived from a channel's inputs, recomputed only when they
/// change.
#[derive(Clone, Copy, Default)]
pub struct Cached<K, V> {
    key: Option<K>,
    value: V,
}

impl<K: Copy + PartialEq, V: Copy> Cached<K, V> {
    /// The setting for inputs `key`, from `derive` when they differ from
    /// the last.
    #[inline(always)]
    pub fn get(&mut self, key: K, derive: impl FnOnce(K) -> V) -> V {
        if self.key != Some(key) {
            self.key = Some(key);
            self.value = derive(key);
        }
        self.value
    }
}

/// Runs `tick` on each slot of `range`, with that slot's rows of `inputs`,
/// to write the slot's channels of `out` through [`Slot::set`].
#[inline(always)]
pub fn each_slot<const N: usize>(
    out: &mut BlockPort,
    range: Range<usize>,
    inputs: [BlockInput<'_>; N],
    mut tick: impl FnMut([&[f32]; N], Slot<'_>),
) {
    each_slot_of([out], range, inputs, |rows, [slot]| tick(rows, slot));
}

/// [`each_slot`] for a drum with several outputs, `outs`.
#[inline(always)]
pub fn each_slot_of<const N: usize, const M: usize>(
    mut outs: [&mut BlockPort; M],
    range: Range<usize>,
    inputs: [BlockInput<'_>; N],
    mut tick: impl FnMut([&[f32]; N], [Slot<'_>; M]),
) {
    for i in range {
        tick(
            inputs.each_ref().map(|input| input.row(i)),
            outs.each_mut().map(|out| Slot(out.slot_mut(i))),
        );
    }
}

/// One slot's channels of a block output.
pub struct Slot<'a>(&'a mut [f32]);

impl Slot<'_> {
    /// Sets channel `ch`, silencing a non-finite value as every output does.
    #[inline(always)]
    pub fn set(&mut self, ch: usize, value: f32) {
        self.0[ch] = sanitize(value);
    }
}
