//! The V2164 gain cell in class A: exact exponential gain plus a second
//! harmonic from mismatched emitter resistances in the cell's NPN and PNP
//! halves, which vanishes at unity gain.

use super::lanes::{Lanes, f32x4, set_lane};

/// Second-harmonic coefficient, per ampere of input current, fitted to the
/// datasheet's class-A 0.15 % THD at ±20 dB.
const HD2_PER_AMP: f32 = 28.9;

/// A cell's gain with its second-harmonic coefficient `(1 − g) · HD2_PER_AMP`.
#[derive(Clone, Copy)]
pub(super) struct Gain {
    pub(super) gain: f32,
    hd2: f32,
}

impl Gain {
    pub(super) fn new(gain: f32) -> Self {
        Self {
            gain,
            hd2: (1.0 - gain) * HD2_PER_AMP,
        }
    }
}

/// A group's four [`Gain`]s, one per lane.
#[derive(Clone, Copy, Default)]
pub(super) struct GainX4 {
    gain: f32x4,
    hd2: f32x4,
}

impl GainX4 {
    pub(super) fn set(&mut self, lane: usize, g: &Gain) {
        set_lane(&mut self.gain, lane, g.gain);
        set_lane(&mut self.hd2, lane, g.hd2);
    }

    #[inline]
    pub(super) fn gain<L: Lanes>(&self) -> L {
        L::load(&self.gain)
    }
}

/// The cells' gain relative to nominal for input currents `i_in` (A): the
/// output is `gain · i_in · scale`.
#[inline]
pub(super) fn scale<L: Lanes>(gain: &GainX4, i_in: L) -> L {
    L::splat(1.0) + L::load(&gain.hd2) * i_in
}

/// The cells' output currents for input currents `i_in`.
#[inline]
pub(super) fn output<L: Lanes>(gain: &GainX4, i_in: L) -> L {
    gain.gain::<L>() * i_in * scale(gain, i_in)
}
