//! The scaffold every drum voice runs in: trigger detection with its accent
//! sample, the trigger pulse as it plays out, and a channel's sounding and
//! cut-off circuits.

use super::input::Input;
use super::lanes::{Lanes, Mono, Real, c, lanewise};
use crate::dsp::utils::SchmittTrigger;

/// How long a hit cut off by a restart takes to fade out under the next
/// one, seconds.
pub const FADE_SECS: f32 = 2.0e-3;

/// Rising-edge detection on a trigger input.
#[derive(Clone, Copy, Default)]
pub struct Strike {
    trigger: SchmittTrigger,
}

impl Strike {
    /// The accent, 0–1, for a strike on this sample, sampling channel
    /// `ch` of `accent` (0–5 V).
    #[inline]
    pub fn process(&mut self, trigger: f32, accent: impl Input, ch: usize) -> Option<f32> {
        self.trigger.process(trigger).then(|| knob(accent.at(ch)))
    }
}

/// A trigger pulse as it plays out: its voltage for the remaining samples.
#[derive(Clone, Copy, Default)]
pub struct Pulse<L: Lanes = Mono> {
    left: L::S,
    volts: L::S,
}

lanewise!(Pulse { left, volts });

impl Pulse {
    pub fn fire(&mut self, volts: f32, samples: u32) {
        self.left = samples as f32;
        self.volts = volts;
    }
}

impl<L: Lanes> Pulse<L> {
    pub fn active(&self) -> L::SM {
        self.left.gt(c(0.0))
    }

    #[inline]
    pub fn next(&mut self) -> L::S {
        let on = self.active();
        self.left = L::S::select(on, self.left - 1.0, self.left);
        L::S::select(on, self.volts, c(0.0))
    }
}

/// How far a 0–5 V control input turns its knob, 0–1.
#[inline]
pub fn knob(volts: f32) -> f32 {
    volts.clamp(0.0, 5.0) / 5.0
}

/// Samples in `secs` at `sample_rate`, at least one.
pub fn samples(secs: f32, sample_rate: f32) -> u32 {
    (secs * sample_rate).round().max(1.0) as u32
}

/// Per-sample-rate timing shared by every channel of a voice.
#[derive(Clone, Copy, Default)]
pub struct Timing {
    /// The machine's trigger pulse.
    pub trigger_samples: u32,
    pub fade_samples: u32,
    /// [`quiet_fall`] at the sample rate.
    pub quiet_fall: f32,
}

impl Timing {
    /// The timing for a machine whose trigger pulse lasts `trigger_secs`.
    pub fn new(trigger_secs: f32, sample_rate: f32) -> Self {
        Self {
            trigger_samples: samples(trigger_secs, sample_rate),
            fade_samples: samples(FADE_SECS, sample_rate),
            quiet_fall: quiet_fall(sample_rate),
        }
    }
}

/// A voice circuit's state through a sequence of hits, at width `L`. The
/// default value is the circuit at rest.
pub trait Circuit<L: Lanes = Mono>: Copy + Default {
    /// Constants shared by every channel, fixed by the sample rate.
    type Shared;
    /// Settings read once per sample.
    type Controls;

    /// True once the circuit has come to rest and outputs silence.
    fn settled(&self) -> L::SM;
    /// Advances one sample, returning the voice's output before its final
    /// gain.
    fn step(&mut self, shared: &Self::Shared, controls: &Self::Controls) -> L::S;
}

/// Starting a hit, which happens one channel at a time.
pub trait Fire: Circuit {
    /// Starts a hit with `accent`, 0–1.
    fn fire(&mut self, accent: f32, shared: &Self::Shared);

    /// The output scale of a fully accented hit, which brings the circuit's
    /// own response to accent to twice the level of an unaccented hit.
    #[inline(always)]
    fn accent_trim(_shared: &Self::Shared) -> f32 {
        1.0
    }
}

/// `voice` with a cut-off `tail` fading out under it, never louder than the
/// louder of the two, so a hit struck over a sounding one peaks no higher
/// than either.
#[inline(always)]
pub fn under<R: Real>(voice: R, tail: R) -> R {
    let bound = voice.abs().max(tail.abs());
    (voice + tail).max(-bound).min(bound)
}

/// The output stage's swing: linear to ±10 V, then easing toward ±12 V, so
/// no setting or retrigger rate drives a voice far past its range.
#[inline(always)]
pub fn rails(v: f32) -> f32 {
    const KNEE: f32 = 10.0;
    const ROOM: f32 = 2.0;
    let a = v.abs();
    if a <= KNEE {
        v
    } else {
        (KNEE + ROOM * ((a - KNEE) / ROOM).tanh()).copysign(v)
    }
}

/// One output channel: the strike detector, the sounding circuit, and a
/// cut-off one fading out under it, each with its accent trim less one.
#[derive(Clone, Copy, Default)]
pub struct Voices<C> {
    pub(super) strike: Strike,
    pub(super) voice: C,
    pub(super) tail: C,
    pub(super) tail_left: u32,
    pub(super) trim: Level,
    pub(super) tail_trim: f32,
}

impl<C: Fire> Voices<C> {
    /// Runs one sample: a rising `trigger` strikes the circuit, sampling
    /// `accent`; with `restart`, a still-sounding hit moves to the tail and
    /// fades out over `fade_samples` while a fresh circuit takes the strike.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn process(
        &mut self,
        trigger: f32,
        accent: impl Input,
        ch: usize,
        restart: bool,
        fade_samples: u32,
        shared: &C::Shared,
        controls: &C::Controls,
    ) -> f32 {
        let trim = C::accent_trim(shared) - 1.0;
        if let Some(accent) = self.strike.process(trigger, accent, ch) {
            if restart && !self.voice.settled() {
                self.tail = self.voice;
                self.tail_left = fade_samples;
                self.tail_trim = self.trim.now();
                self.voice = C::default();
            }
            if trim != 0.0 {
                self.trim
                    .set(trim * accent, !self.voice.settled(), fade_samples);
            }
            self.voice.fire(accent, shared);
        }
        let mut out = if self.voice.settled() {
            0.0
        } else {
            self.voice.step(shared, controls)
        };
        if trim != 0.0 {
            out *= 1.0 + self.trim.next();
        }
        if self.tail_left > 0 {
            let fade = self.tail_left as f32 / fade_samples as f32;
            self.tail_left -= 1;
            let mut tail = fade * self.tail.step(shared, controls);
            if trim != 0.0 {
                tail *= 1.0 + self.tail_trim;
            }
            out = under(out, tail);
        }
        out
    }
}

/// A running peak of a voice's output, falling 60 dB in `QUIET_SECS`, so
/// silence means silent for a while rather than a zero crossing.
#[derive(Clone, Copy, Default)]
pub struct Quiet<L: Lanes = Mono> {
    level: L::S,
}

lanewise!(Quiet { level });

const QUIET_SECS: f32 = 0.03;

/// Per-sample fall of [`Quiet`]'s peak at `sample_rate`.
pub fn quiet_fall(sample_rate: f32) -> f32 {
    (-std::f32::consts::LN_10 * 3.0 / (QUIET_SECS * sample_rate)).exp()
}

impl<L: Lanes> Quiet<L> {
    /// Takes in a sample, with the peak falling by `fall` from
    /// [`quiet_fall`].
    #[inline]
    pub fn track(&mut self, out: L::S, fall: f32) {
        self.level = (self.level * fall).max(out.abs());
    }

    /// True once the peak has fallen below `threshold`.
    pub fn below(&self, threshold: f32) -> L::SM {
        self.level.lt(c(threshold))
    }
}

/// A voice's output scale, set by each hit's accent. A hit on a sounding
/// voice ramps it to the new scale, so the tail it rescales does not click.
#[derive(Clone, Copy, Default)]
pub struct Level<L: Lanes = Mono> {
    now: L::S,
    step: L::S,
    left: L::S,
}

lanewise!(Level { now, step, left });

impl Level {
    /// Moves to `target`, over `samples` while `sounding`, at once otherwise.
    pub fn set(&mut self, target: f32, sounding: bool, samples: u32) {
        if sounding {
            self.step = (target - self.now) / samples as f32;
            self.left = samples as f32;
        } else {
            self.now = target;
            self.left = 0.0;
        }
    }
}

impl<L: Lanes> Level<L> {
    pub fn now(&self) -> L::S {
        self.now
    }

    #[inline]
    pub fn next(&mut self) -> L::S {
        let on = self.left.gt(c(0.0));
        self.left = L::S::select(on, self.left - 1.0, self.left);
        self.now = L::S::select(on, self.now + self.step, self.now);
        self.now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_ramps_only_while_sounding() {
        let mut level = Level::default();
        level.set(2.0, false, 96);
        assert_eq!(level.next(), 2.0);
        level.set(1.0, true, 96);
        let ramp: Vec<f32> = (0..100).map(|_| level.next()).collect();
        assert!(ramp.windows(2).all(|w| (w[0] - w[1]).abs() < 1.1 / 96.0));
        assert!((ramp[95] - 1.0).abs() < 1.0e-5 && ramp[99] == ramp[95]);
    }

    #[test]
    fn quiet_falls_60_db_in_its_window_at_any_rate() {
        for sample_rate in [24_000.0f32, 48_000.0, 96_000.0] {
            let mut quiet = Quiet::<Mono>::default();
            let fall = quiet_fall(sample_rate);
            quiet.track(1.0, fall);
            for _ in 0..(QUIET_SECS * sample_rate) as usize - 1 {
                quiet.track(0.0, fall);
            }
            assert!(
                !quiet.below(0.99e-3) && quiet.below(1.01e-3),
                "{sample_rate}"
            );
        }
    }
}
