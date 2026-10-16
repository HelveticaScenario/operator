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

/// A running peak of a voice's output, falling by `QUIET_FALL` each sample,
/// so silence means silent for a while rather than a zero crossing.
#[derive(Clone, Copy, Default)]
pub struct Quiet {
    level: f32,
}

/// Per-sample fall of [`Quiet`]'s peak: about −60 dB in 30 ms at 48 kHz.
const QUIET_FALL: f32 = 0.9952;

impl Quiet {
    #[inline]
    pub fn track(&mut self, out: f32) {
        self.level = (self.level * QUIET_FALL).max(out.abs());
    }

    /// True once the peak has fallen below `threshold`.
    pub fn below(&self, threshold: f32) -> bool {
        self.level < threshold
    }
}
