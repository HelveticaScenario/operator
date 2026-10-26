//! Recording path and grain playback for one voice at the grain engine's
//! sample rate: input plus feedback is written to the voice's buffer, and
//! its grains read from it.

use super::buffer::{GrainBuffer, Linear12Codec, Linear16Codec, MuLaw8Codec, Resolution};
use super::feedback::FeedbackPath;
use super::grain::Grain;
use super::player::{GrainTrigger, GranularPlayer};
use super::primitives::Frame;

const MAX_FEEDBACK_GAIN: f32 = 3.0;

/// A grain start with its gain and pan resolved across all voices.
#[derive(Clone, Copy, Debug)]
pub struct GrainStartRequest {
    pub trigger: GrainTrigger,
    pub gain: f32,
    pub pan: f32,
}

#[derive(Default)]
pub struct GranularCore {
    pub buffer: GrainBuffer,
    pub player: GranularPlayer,
    feedback: FeedbackPath,
}

impl GranularCore {
    /// Configure for a new quality, clearing the buffer and all grains.
    pub fn reset(&mut self, sample_rate: f32, resolution: Resolution, max_delay: usize) {
        self.buffer.reset(resolution, max_delay);
        self.player.init(sample_rate);
        self.feedback
            .reset(sample_rate, resolution, MAX_FEEDBACK_GAIN);
    }

    /// Allocate a buffer of `frames` samples. Main thread only.
    pub fn allocate_frames(&mut self, frames: usize) {
        self.buffer.allocate_frames(frames);
    }

    /// Record `input` through the recording path with no feedback.
    pub fn load(&mut self, input: f32) {
        self.record(input, false, None);
    }

    pub fn start_block(&mut self, feedback: f32, block_size: usize) {
        self.feedback.start_block(feedback, block_size);
    }

    /// Record `input` plus feedback, and start `start` if given.
    #[inline]
    fn record(&mut self, input: f32, freeze: bool, start: Option<GrainStartRequest>) {
        let s = self.feedback.record(input);
        self.buffer.write(s, freeze);
        if let Some(r) = start {
            let t = r.trigger;
            self.player
                .start(&self.buffer, &t.params, t.fractional_sample, r.gain, r.pan);
        }
    }

    /// Record `input`, start `start` if given, and return this voice's grains
    /// from a source placed at `place`, with their unpanned sum, which also
    /// feeds back.
    pub fn process(
        &mut self,
        input: f32,
        freeze: bool,
        start: Option<GrainStartRequest>,
        place: [f32; 2],
    ) -> (Frame, f32) {
        self.record(input, freeze, start);
        let mut out = Frame::ZERO;
        let mut mono = 0.0;
        let mut mix = |g: &Grain, x: f32| g.mix(x, place, &mut out, &mut mono);
        let buffer = &self.buffer;
        match buffer.resolution() {
            Resolution::MuLaw8 => self.player.render::<MuLaw8Codec>(buffer, &mut mix),
            Resolution::Linear12 => self.player.render::<Linear12Codec>(buffer, &mut mix),
            Resolution::Linear16 => self.player.render::<Linear16Codec>(buffer, &mut mix),
        }
        self.feedback.feed_back(mono);
        (out, mono)
    }
}
