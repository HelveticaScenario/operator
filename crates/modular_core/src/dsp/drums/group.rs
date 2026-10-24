//! Running a drum's channels in groups. Channels form blocks of four from
//! the first. A block of three or four channels runs as one [`X4`]
//! circuit, or as [`X2`] pairs, and a block of two as one pair, each where
//! the drum runs faster that way; every other channel runs alone at
//! [`Mono`] width.
//!
//! Each channel holds the [`Mono`] form of its own state. While its group
//! runs, the group holds the live form, and a patch update that changes
//! the channel count hands every group back to its channels and gathers
//! the new groups from them.

use super::input::Input;
use super::lanes::{Lanes, Lanewise, Mask, Mono, Real, X2, X4, c};
use super::voice::{Circuit, Fire, Level, Voices, under};
use crate::poly::PORT_MAX_CHANNELS;

/// Channels in a block, and in a full group.
pub const QUAD: usize = 4;

/// How a run of channels is grouped.
#[derive(Clone, Copy)]
enum Width {
    One,
    Pair,
    Quad,
}

/// Hands `each` the groups `channels` channels of drum `D` run in, as each
/// one's first channel, number of channels, and width.
#[inline(always)]
fn groups<D: Drum>(channels: usize, mut each: impl FnMut(usize, usize, Width)) {
    #[cfg(test)]
    if ALONE.get() {
        (0..channels).for_each(|ch| each(ch, 1, Width::One));
        return;
    }
    for first in (0..channels).step_by(QUAD) {
        let lanes = (channels - first).min(QUAD);
        if lanes > 2 && D::RUNS_IN_QUADS {
            each(first, lanes, Width::Quad);
        } else if lanes > 1 && D::RUNS_IN_PAIRS {
            each(first, 2, Width::Pair);
            match lanes {
                4 => each(first + 2, 2, Width::Pair),
                3 => each(first + 2, 1, Width::One),
                _ => {}
            }
        } else {
            for ch in first..first + lanes {
                each(ch, 1, Width::One);
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    /// While set, every channel on this thread runs alone, so a test can
    /// compare each channel against its own grouped run.
    pub static ALONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A drum voice's circuit at every width.
pub trait Drum: 'static {
    /// Constants shared by every channel, fixed by the sample rate.
    type Shared;
    type Circuit<L: Lanes>: Circuit<L, Shared = Self::Shared, Controls = Self::Controls<L>>
        + Lanewise<Lane = Self::Circuit<Mono>>;
    /// Settings read once per sample.
    type Controls<L: Lanes>: Default + Lanewise<Lane = Self::Controls<Mono>>;
    /// State a group runs every sample for its channels besides their
    /// circuits, such as the sources the settings come from. Each channel
    /// holds its own, in the [`Mono`] form, while it runs alone.
    type Sources<L: Lanes>: Default;
    /// Whether three or four channels run faster as one [`X4`] group than
    /// one at a time.
    const RUNS_IN_QUADS: bool;
    /// Whether two channels run faster as one [`X2`] pair than one at a time.
    const RUNS_IN_PAIRS: bool;
}

/// A drum module's channels, as the module reads them each sample.
pub trait ChannelInputs<D: Drum> {
    /// One channel's state.
    type Channel;

    /// The channel's strike detector and circuits.
    fn voices(channel: &mut Self::Channel) -> &mut Voices<D::Circuit<Mono>>;
    /// Channel `ch`'s trigger and settings this sample, running alone.
    fn inputs(&self, ch: usize, channel: &mut Self::Channel) -> (f32, D::Controls<Mono>);

    /// The triggers and settings of a group of channels, `members`, from
    /// `first` on, with the group's `sources`; by default each channel's
    /// [`Self::inputs`], lane by lane.
    #[inline]
    fn group_inputs<L: Lanes>(
        &self,
        first: usize,
        members: &mut [Self::Channel],
        _sources: &mut D::Sources<L>,
        triggers: &mut [f32; QUAD],
    ) -> D::Controls<L> {
        let mut controls = D::Controls::<L>::default();
        for (lane, channel) in members.iter_mut().enumerate() {
            let (trigger, lane_controls) = self.inputs(first + lane, channel);
            triggers[lane] = trigger;
            controls.set_lane(lane, &lane_controls);
        }
        controls
    }

    /// Reads a channel running alone after each of its samples.
    #[inline]
    fn stepped(_channel: &mut Self::Channel) {}

    /// Reads a group's sounding circuits, `voice`, after they step, with
    /// `sounding` marking the lanes of `members` they sound for.
    #[inline]
    fn group_stepped<L: Lanes>(
        _members: &mut [Self::Channel],
        _voice: &D::Circuit<L>,
        _sounding: u32,
    ) {
    }

    /// Moves `channel`'s sources into lane `lane` of a group's.
    #[inline]
    fn load_sources<L: Lanes>(
        _channel: &mut Self::Channel,
        _lane: usize,
        _sources: &mut D::Sources<L>,
    ) {
    }

    /// Moves lane `lane` of a group's sources back into `channel`.
    #[inline]
    fn unload_sources<L: Lanes>(
        _channel: &mut Self::Channel,
        _lane: usize,
        _sources: &D::Sources<L>,
    ) {
    }
}

/// The settings every channel of a drum runs with this sample.
pub struct Run<'a, D: Drum, I: Input> {
    pub accent: I,
    pub restart: bool,
    pub fade_samples: u32,
    pub shared: &'a D::Shared,
}

/// A drum's groups, by their first channel: a group of four from each
/// block's first, and a pair from each block's first and third.
pub struct Groups<D: Drum> {
    /// The channel count the groups are laid out for.
    channels: usize,
    quads: Box<[VoiceGroup<X4, D>]>,
    pairs: Box<[VoiceGroup<X2, D>]>,
}

impl<D: Drum> Default for Groups<D> {
    fn default() -> Self {
        Self {
            channels: 0,
            quads: Box::default(),
            pairs: Box::default(),
        }
    }
}

impl<D: Drum> Groups<D>
where
    D::Circuit<Mono>: Fire,
{
    /// Groups for every channel count a module can have. Allocates, so it
    /// belongs in `init`.
    pub fn new() -> Self {
        let quads = if D::RUNS_IN_QUADS {
            PORT_MAX_CHANNELS / QUAD
        } else {
            0
        };
        let pairs = if D::RUNS_IN_PAIRS {
            PORT_MAX_CHANNELS / 2
        } else {
            0
        };
        Self {
            channels: 0,
            quads: (0..quads).map(|_| VoiceGroup::default()).collect(),
            pairs: (0..pairs).map(|_| VoiceGroup::default()).collect(),
        }
    }

    /// Lays out `count` channels, gathering each channel's state from where
    /// the last layout left it.
    pub fn layout<A: ChannelInputs<D>>(&mut self, channels: &mut [A::Channel], count: usize) {
        groups::<D>(self.channels, |first, lanes, width| {
            let kept = &mut channels[first.min(count)..(first + lanes).min(count)];
            match width {
                Width::One => {}
                Width::Pair => self.pairs[first / 2].unload::<A>(kept),
                Width::Quad => self.quads[first / QUAD].unload::<A>(kept),
            }
        });
        groups::<D>(count, |first, lanes, width| {
            let members = &mut channels[first..first + lanes];
            match width {
                Width::One => {}
                Width::Pair => self.pairs[first / 2].load::<A>(members),
                Width::Quad => self.quads[first / QUAD].load::<A>(members),
            }
        });
        self.channels = count;
    }

    /// Runs one sample of every channel, handing `out` each channel's
    /// output.
    #[inline]
    pub fn run<A: ChannelInputs<D>, I: Input>(
        &mut self,
        adapter: &A,
        channels: &mut [A::Channel],
        run: &Run<D, I>,
        mut out: impl FnMut(usize, f32),
    ) {
        groups::<D>(channels.len(), |first, lanes, width| {
            let members = &mut channels[first..first + lanes];
            match width {
                Width::One => {
                    let channel = &mut members[0];
                    let (trigger, controls) = adapter.inputs(first, channel);
                    let v = A::voices(channel).process(
                        trigger,
                        run.accent,
                        first,
                        run.restart,
                        run.fade_samples,
                        run.shared,
                        &controls,
                    );
                    A::stepped(channel);
                    out(first, v);
                }
                Width::Pair => self.pairs[first / 2].run(adapter, members, first, run, &mut out),
                Width::Quad => self.quads[first / QUAD].run(adapter, members, first, run, &mut out),
            }
        });
    }
}

/// A group's circuits at width `L`: the sounding circuits and the cut-off
/// ones fading out under them. A lane whose circuit settles is parked: its
/// channel's [`Voices`] holds the circuit as it settled, as a channel
/// running alone would, while the lane runs on unheard, and the channel's
/// next hit starts from the parked circuit.
struct VoiceGroup<L: Lanes, D: Drum> {
    voice: D::Circuit<L>,
    tail: D::Circuit<L>,
    trim: Level<L>,
    tail_trim: L::S,
    parked: u32,
    sources: D::Sources<L>,
}

impl<L: Lanes, D: Drum> Default for VoiceGroup<L, D> {
    fn default() -> Self {
        Self {
            voice: Default::default(),
            tail: Default::default(),
            trim: Default::default(),
            tail_trim: Default::default(),
            parked: u32::MAX,
            sources: Default::default(),
        }
    }
}

impl<L: Lanes, D: Drum> VoiceGroup<L, D>
where
    D::Circuit<Mono>: Fire,
{
    /// Takes up the circuits of `members`, one channel per lane. Lanes past
    /// them rest.
    fn load<A: ChannelInputs<D>>(&mut self, members: &mut [A::Channel]) {
        *self = Self::default();
        for (lane, channel) in members.iter_mut().enumerate() {
            A::load_sources(channel, lane, &mut self.sources);
            let v = A::voices(channel);
            self.voice.set_lane(lane, &v.voice);
            self.tail.set_lane(lane, &v.tail);
            self.trim.set_lane(lane, &v.trim);
            self.tail_trim.set_lane(lane, &v.tail_trim);
            if !v.voice.settled() {
                self.parked &= !(1 << lane);
            }
        }
    }

    /// Hands each lane's circuits back to its channel in `members`.
    fn unload<A: ChannelInputs<D>>(&self, members: &mut [A::Channel]) {
        for (lane, channel) in members.iter_mut().enumerate() {
            A::unload_sources(channel, lane, &self.sources);
            let v = A::voices(channel);
            if self.parked & (1 << lane) == 0 {
                v.voice = self.voice.lane(lane);
            }
            v.tail = self.tail.lane(lane);
            v.trim = self.trim.lane(lane);
            v.tail_trim = self.tail_trim.lane(lane);
        }
    }

    /// Runs one sample of each channel in `members`, from `first` on, as
    /// [`Voices::process`] does for one channel, handing `out` each
    /// channel's output.
    #[inline(never)]
    fn run<A: ChannelInputs<D>, I: Input>(
        &mut self,
        adapter: &A,
        members: &mut [A::Channel],
        first: usize,
        run: &Run<D, I>,
        out: &mut impl FnMut(usize, f32),
    ) {
        let live = (1u32 << members.len()) - 1;
        let trim = <D::Circuit<Mono> as Fire>::accent_trim(run.shared) - 1.0;
        let mut triggers = [0.0; QUAD];
        let mut controls = adapter.group_inputs(first, members, &mut self.sources, &mut triggers);
        for (lane, channel) in members.iter_mut().enumerate() {
            let v = A::voices(channel);
            let Some(accent) = v.strike.process(triggers[lane], run.accent, first + lane) else {
                continue;
            };
            let mut circuit = if self.parked & (1 << lane) != 0 {
                v.voice
            } else {
                self.voice.lane(lane)
            };
            if run.restart && !circuit.settled() {
                self.tail.set_lane(lane, &circuit);
                self.tail_trim.set_lane(lane, &self.trim.lane(lane).now());
                v.tail_left = run.fade_samples;
                circuit = Default::default();
            }
            if trim != 0.0 {
                let mut lane_trim = self.trim.lane(lane);
                lane_trim.set(trim * accent, !circuit.settled(), run.fade_samples);
                self.trim.set_lane(lane, &lane_trim);
            }
            circuit.fire(accent, run.shared);
            self.voice.set_lane(lane, &circuit);
            self.parked &= !(1 << lane);
        }
        // Lanes past the group's channels follow its first, so they run on
        // settings a channel could have.
        let first_controls = controls.lane(0);
        for lane in members.len()..L::WIDTH {
            controls.set_lane(lane, &first_controls);
        }

        let settled = self.voice.settled().bits() & live & !self.parked;
        if settled != 0 {
            for (lane, channel) in members.iter_mut().enumerate() {
                if settled & (1 << lane) != 0 {
                    A::voices(channel).voice = self.voice.lane(lane);
                }
            }
            self.parked |= settled;
        }

        let sounding = live & !self.parked;
        let mut y = c::<L::S>(0.0);
        if sounding != 0 {
            let step = self.voice.step(run.shared, &controls);
            y = L::S::select(L::mask(sounding), step, y);
        }
        A::group_stepped(members, &self.voice, sounding);
        if trim != 0.0 {
            y *= c::<L::S>(1.0) + self.trim.next();
        }
        let mut tails = 0;
        let mut fade = c::<L::S>(0.0);
        for (lane, channel) in members.iter_mut().enumerate() {
            let v = A::voices(channel);
            if v.tail_left > 0 {
                tails |= 1 << lane;
                fade.set_lane(lane, &(v.tail_left as f32 / run.fade_samples as f32));
                v.tail_left -= 1;
            }
        }
        if tails != 0 {
            let mut tail = fade * self.tail.step(run.shared, &controls);
            if trim != 0.0 {
                tail *= c::<L::S>(1.0) + self.tail_trim;
            }
            y = L::S::select(L::mask(tails), under(y, tail), y);
        }
        for lane in 0..members.len() {
            out(first + lane, y.lane(lane));
        }
    }
}
