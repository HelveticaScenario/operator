//! A drum's inputs for the sample it runs, read live by `update` and from
//! resolved block rows by `process_block`, so both run the same per-sample
//! code.

use std::ops::Range;

use crate::BlockPort;
use crate::block_input::BlockInput;
use crate::dsp::utils::{SchmittTrigger, sanitize};
use crate::poly::{PORT_MAX_CHANNELS, PolySignal};

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

/// `N` control inputs held from strike to strike: while holding, each
/// channel reads the values they had on its last rising edge on any of `T`
/// trigger inputs, detected as its voice detects a strike.
#[derive(Clone, Copy)]
pub struct Hold<const T: usize, const N: usize> {
    edges: [[SchmittTrigger; T]; PORT_MAX_CHANNELS],
    values: [[f32; N]; PORT_MAX_CHANNELS],
    /// Whether each channel's `values` have been taken since holding began.
    taken: [bool; PORT_MAX_CHANNELS],
    holding: bool,
}

impl<const T: usize, const N: usize> Default for Hold<T, N> {
    fn default() -> Self {
        Self {
            edges: [[SchmittTrigger::default(); T]; PORT_MAX_CHANNELS],
            values: [[0.0; N]; PORT_MAX_CHANNELS],
            taken: [false; PORT_MAX_CHANNELS],
            holding: false,
        }
    }
}

impl<const T: usize, const N: usize> Hold<T, N> {
    /// The `controls` the first `channels` channels read this sample, given
    /// this sample's `triggers`: while `hold`, their values on each channel's
    /// last strike, or on the sample holding began; otherwise live.
    #[inline(always)]
    pub fn apply<I: Input>(
        &mut self,
        hold: bool,
        channels: usize,
        triggers: [I; T],
        controls: [I; N],
    ) -> [Held<'_, I, N>; N] {
        if !hold {
            if self.holding {
                self.holding = false;
                self.taken = [false; PORT_MAX_CHANNELS];
            }
            return controls.map(Held::Live);
        }
        self.holding = true;
        for ch in 0..channels {
            let mut struck = false;
            for (edge, trigger) in self.edges[ch].iter_mut().zip(triggers) {
                struck |= edge.process(trigger.at(ch));
            }
            if struck || !self.taken[ch] {
                self.taken[ch] = true;
                self.values[ch] = controls.map(|control| control.at(ch));
            }
        }
        std::array::from_fn(|k| Held::Stored(&self.values, k))
    }
}

/// An input read live, or from the values a [`Hold`] took.
#[derive(Clone, Copy)]
pub enum Held<'a, I, const N: usize> {
    Live(I),
    Stored(&'a [[f32; N]; PORT_MAX_CHANNELS], usize),
}

impl<I: Input, const N: usize> Input for Held<'_, I, N> {
    #[inline(always)]
    fn at(self, ch: usize) -> f32 {
        match self {
            Self::Live(input) => input.at(ch),
            Self::Stored(values, k) => values[ch][k],
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample of `hold` over two channels, with each channel's two
    /// triggers in `triggers` and its control in `control`.
    fn read(
        hold: &mut Hold<2, 1>,
        on: bool,
        triggers: [[f32; 2]; 2],
        control: [f32; 2],
    ) -> [f32; 2] {
        let first = [triggers[0][0], triggers[1][0]];
        let second = [triggers[0][1], triggers[1][1]];
        let [held] = hold.apply(on, 2, [&first[..], &second[..]], [&control[..]]);
        [held.at(0), held.at(1)]
    }

    #[test]
    fn holds_from_strike_to_strike_on_either_trigger() {
        let mut hold = Hold::default();
        let quiet = [[0.0; 2]; 2];
        // Holding begins with the controls of that sample.
        assert_eq!(read(&mut hold, true, quiet, [1.0, 2.0]), [1.0, 2.0]);
        assert_eq!(read(&mut hold, true, quiet, [3.0, 4.0]), [1.0, 2.0]);
        // A strike on channel 0's first trigger takes channel 0's control.
        let first = [[5.0, 0.0], [0.0, 0.0]];
        assert_eq!(read(&mut hold, true, first, [5.0, 6.0]), [5.0, 2.0]);
        // One on channel 1's second trigger takes channel 1's.
        let second = [[5.0, 0.0], [0.0, 5.0]];
        assert_eq!(read(&mut hold, true, second, [7.0, 8.0]), [5.0, 8.0]);
        // Released, the controls read live; holding again starts afresh.
        assert_eq!(read(&mut hold, false, quiet, [9.0, 10.0]), [9.0, 10.0]);
        assert_eq!(read(&mut hold, true, quiet, [11.0, 12.0]), [11.0, 12.0]);
        assert_eq!(read(&mut hold, true, quiet, [13.0, 14.0]), [11.0, 12.0]);
    }
}
