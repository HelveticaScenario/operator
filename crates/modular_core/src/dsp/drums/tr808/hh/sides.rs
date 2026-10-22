//! The hi-hat's two voices as drums of their own: the closed hat, which
//! runs the channel's oscillator bank and Q23, and the open hat, which reads
//! them through its channel after the closed hat steps.

use super::*;
use crate::dsp::drums::group::{Channels, Drum, QUAD};
use crate::dsp::drums::input::{Cached, Input};
use crate::dsp::drums::lanes::Lanewise;
use crate::dsp::drums::tr808::metal::{MetalBank, MetalFilters};
use crate::dsp::drums::voice::Voices;

/// The decay pot, VR3: 2 MΩ linear, after R132.
const R132: f32 = 100.0e3;
const VR3: f32 = 2.0e6;

/// One channel of both hats.
#[derive(Clone, Copy, Default)]
pub(super) struct Channel {
    closed: Voices<Closed>,
    open: Voices<Open>,
    bank: MetalBank,
    cut: Cached<f32, (f32, f32)>,
    /// This sample's upper band, Q23's base current, and the resistance Q23
    /// drains the open hat through, which the closed hat hands the open.
    band: f32,
    q23: f32,
    drain: f32,
}

impl Channel {
    pub(super) fn seed(&mut self, base: u64, ch: usize) {
        self.bank.seed(base, ch);
    }
}

/// The closed hat at every width, with its oscillator bank.
pub(super) struct ClosedHat;

impl Drum for ClosedHat {
    type Shared = HatShared;
    type Circuit<L: Lanes> = Closed<L>;
    type Controls<L: Lanes> = ClosedControls<L>;
    type Sources<L: Lanes> = MetalBank<L>;
    const QUAD: bool = true;
    const PAIR: bool = false;
}

/// The open hat at every width.
pub(super) struct OpenHat;

impl Drum for OpenHat {
    type Shared = HatShared;
    type Circuit<L: Lanes> = Open<L>;
    type Controls<L: Lanes> = OpenControls<L>;
    type Sources<L: Lanes> = ();
    const QUAD: bool = true;
    const PAIR: bool = true;
}

/// The inputs every channel of the closed hat reads.
pub(super) struct ClosedSide<'a, I> {
    pub trigger: I,
    pub cut: I,
    pub filters: &'a MetalFilters,
}

impl<I: Input> ClosedSide<'_, I> {
    /// Channel `ch`'s Q23 base conductance, keeping its drain resistance
    /// for the open hat.
    #[inline(always)]
    fn cut(&self, ch: usize, cs: &mut Channel) -> f32 {
        let (q23, drain) = cs.cut.get(self.cut.at(ch), cut);
        cs.drain = drain;
        q23
    }
}

impl<I: Input> Channels<ClosedHat> for ClosedSide<'_, I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Closed> {
        &mut channel.closed
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, cs: &mut Channel) -> (f32, ClosedControls) {
        let (band, _) = cs.bank.step(self.filters);
        cs.band = band;
        let q23 = self.cut(ch, cs);
        (self.trigger.at(ch), ClosedControls { band, q23 })
    }

    #[inline]
    fn group_inputs<L: Lanes>(
        &self,
        first: usize,
        members: &mut [Channel],
        bank: &mut MetalBank<L>,
        triggers: &mut [f32; QUAD],
    ) -> ClosedControls<L> {
        let (band, _) = bank.step(self.filters);
        let mut q23 = c::<L::S>(0.0);
        for (lane, cs) in members.iter_mut().enumerate() {
            let ch = first + lane;
            triggers[lane] = self.trigger.at(ch);
            cs.band = band.lane(lane);
            q23.set_lane(lane, &self.cut(ch, cs));
        }
        ClosedControls { band, q23 }
    }

    #[inline]
    fn stepped(cs: &mut Channel) {
        cs.q23 = cs.closed.voice.q23();
    }

    #[inline]
    fn group_stepped<L: Lanes>(members: &mut [Channel], voice: &Closed<L>, sounding: u32) {
        let q23 = voice.q23();
        for (lane, cs) in members.iter_mut().enumerate() {
            cs.q23 = if sounding & (1 << lane) != 0 {
                q23.lane(lane)
            } else {
                0.0
            };
        }
    }

    fn load_sources<L: Lanes>(cs: &mut Channel, lane: usize, bank: &mut MetalBank<L>) {
        bank.set_lane(lane, &cs.bank);
    }

    fn unload_sources<L: Lanes>(cs: &mut Channel, lane: usize, bank: &MetalBank<L>) {
        cs.bank = bank.lane(lane);
    }
}

/// The inputs every channel of the open hat reads.
pub(super) struct OpenSide<I> {
    pub trigger: I,
    pub decay: I,
}

impl<I: Input> Channels<OpenHat> for OpenSide<I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Open> {
        &mut channel.open
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, cs: &mut Channel) -> (f32, OpenControls) {
        let decay = self.decay.at(ch).clamp(0.0, 5.0) / 5.0;
        let controls = OpenControls {
            band: cs.band,
            decay: 1.0 / (R132 + VR3 * decay),
            q23: cs.q23,
            cut: cs.drain,
        };
        (self.trigger.at(ch), controls)
    }
}
