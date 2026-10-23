//! The scaffold every drum voice runs in: trigger detection with its accent
//! sample, the trigger pulse as it plays out, and a channel's sounding and
//! choked circuits.

use crate::dsp::utils::SchmittTrigger;
use crate::poly::{PolySignal, PolySignalExt};

/// How long a choked hit takes to fade out under the next one, seconds.
pub const CHOKE_SECS: f32 = 2.0e-3;

/// Rising-edge detection on a trigger input.
#[derive(Clone, Copy, Default)]
pub struct Strike {
    trigger: SchmittTrigger,
}

impl Strike {
    /// The accent, 0–1, for a strike on this sample, sampling `accent`
    /// (0–5 V).
    #[inline]
    pub fn process(&mut self, trigger: f32, accent: &Option<PolySignal>, ch: usize) -> Option<f32> {
        self.trigger
            .process(trigger)
            .then(|| accent.value_or(ch, 0.0).clamp(0.0, 5.0) / 5.0)
    }
}

/// A trigger pulse as it plays out: its voltage for the remaining samples.
#[derive(Clone, Copy, Default)]
pub struct Pulse {
    left: u32,
    volts: f32,
}

impl Pulse {
    pub fn fire(&mut self, volts: f32, samples: u32) {
        self.left = samples;
        self.volts = volts;
    }

    pub fn active(&self) -> bool {
        self.left > 0
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.left > 0 {
            self.left -= 1;
            self.volts
        } else {
            0.0
        }
    }
}

/// Samples in `secs` at `sample_rate`, at least one.
pub fn samples(secs: f32, sample_rate: f32) -> u32 {
    (secs * sample_rate).round().max(1.0) as u32
}

/// A voice circuit's state through a sequence of hits. The default value is
/// the circuit at rest.
pub trait Circuit: Copy + Default {
    /// Constants shared by every channel, fixed by the sample rate.
    type Shared;
    /// Settings read once per sample.
    type Controls;

    /// Starts a hit with `accent`, 0–1.
    fn fire(&mut self, accent: f32, shared: &Self::Shared);
    /// True once the circuit has come to rest and outputs silence.
    fn settled(&self) -> bool;
    /// Advances one sample, returning the voice's output before its final
    /// gain.
    fn step(&mut self, shared: &Self::Shared, controls: &Self::Controls) -> f32;
}

/// One output channel: the strike detector, the sounding circuit, and a
/// choked one fading out under it.
#[derive(Clone, Copy, Default)]
pub struct Voices<C> {
    strike: Strike,
    voice: C,
    tail: C,
    tail_left: u32,
}

impl<C: Circuit> Voices<C> {
    /// Runs one sample: a rising `trigger` strikes the circuit, sampling
    /// `accent`; with `choke`, a still-sounding hit moves to the tail and
    /// fades out over `choke_samples` while a fresh circuit takes the strike.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn process(
        &mut self,
        trigger: f32,
        accent: &Option<PolySignal>,
        ch: usize,
        choke: bool,
        choke_samples: u32,
        shared: &C::Shared,
        controls: &C::Controls,
    ) -> f32 {
        if let Some(accent) = self.strike.process(trigger, accent, ch) {
            if choke && !self.voice.settled() {
                self.tail = self.voice;
                self.tail_left = choke_samples;
                self.voice = C::default();
            }
            self.voice.fire(accent, shared);
        }
        let mut out = if self.voice.settled() {
            0.0
        } else {
            self.voice.step(shared, controls)
        };
        if self.tail_left > 0 {
            let fade = self.tail_left as f32 / choke_samples as f32;
            self.tail_left -= 1;
            out += fade * self.tail.step(shared, controls);
        }
        out
    }
}

/// A running peak of a voice's output, falling 60 dB in `QUIET_SECS`, so
/// silence means silent for a while rather than a zero crossing.
#[derive(Clone, Copy, Default)]
pub struct Quiet {
    level: f32,
}

const QUIET_SECS: f32 = 0.03;

/// Per-sample fall of [`Quiet`]'s peak at `sample_rate`.
pub fn quiet_fall(sample_rate: f32) -> f32 {
    (-std::f32::consts::LN_10 * 3.0 / (QUIET_SECS * sample_rate)).exp()
}

impl Quiet {
    /// Takes in a sample, with the peak falling by `fall` from
    /// [`quiet_fall`].
    #[inline]
    pub fn track(&mut self, out: f32, fall: f32) {
        self.level = (self.level * fall).max(out.abs());
    }

    /// True once the peak has fallen below `threshold`.
    pub fn below(&self, threshold: f32) -> bool {
        self.level < threshold
    }
}

/// A voice's output scale, set by each hit's accent. A hit on a sounding
/// voice ramps it to the new scale, so the tail it rescales does not click.
#[derive(Clone, Copy, Default)]
pub struct Level {
    now: f32,
    step: f32,
    left: u32,
}

impl Level {
    /// Moves to `target`, over `samples` while `sounding`, at once otherwise.
    pub fn set(&mut self, target: f32, sounding: bool, samples: u32) {
        if sounding {
            self.step = (target - self.now) / samples as f32;
            self.left = samples;
        } else {
            self.now = target;
            self.left = 0;
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.left > 0 {
            self.left -= 1;
            self.now += self.step;
        }
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
            let mut quiet = Quiet::default();
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
