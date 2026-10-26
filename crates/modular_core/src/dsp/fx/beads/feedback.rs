//! Beads' recording path: the input plus limited, soft-clipped feedback
//! that is written to the buffer, and the filtering that turns the played
//! signal into the next sample's feedback.

use super::buffer::Resolution;
use super::primitives::{Limiter, Ramp, Svf, soft_clip};

#[derive(Default)]
pub struct FeedbackPath {
    feedback: Ramp,
    previous: f32,
    dc_blocker: Svf,
    input_limiter: Limiter,
    /// Low-pass coefficient on the feedback signal.
    lp_coefficient: f32,
    /// Pre-gain into the feedback soft clipper; 0 disables it.
    soft_limit_gain: f32,
    /// Feedback gain at full `feedback`.
    max_gain: f32,
}

impl FeedbackPath {
    pub fn reset(&mut self, sample_rate: f32, resolution: Resolution, max_gain: f32) {
        self.feedback.reset(0.0);
        self.previous = 0.0;
        self.dc_blocker = Svf::default();
        self.dc_blocker.set_f_q(40.0 / sample_rate, 0.5);
        self.input_limiter.init(1.0 / sample_rate);
        (self.lp_coefficient, self.soft_limit_gain) = match resolution {
            Resolution::MuLaw8 => (0.75, 1.75),
            Resolution::Linear12 => (0.9, 1.3),
            Resolution::Linear16 => (0.999, 0.0),
        };
        self.max_gain = max_gain;
    }

    pub fn start_block(&mut self, feedback: f32, block_size: usize) {
        self.feedback.start(feedback, block_size);
    }

    /// The sample to record for `input`, mixed with the fed-back signal.
    #[inline]
    pub fn record(&mut self, input: f32) -> f32 {
        let feedback = self.feedback.next();
        let mut s = input;
        if self.soft_limit_gain != 0.0 {
            s += self.previous * feedback * (self.max_gain / self.soft_limit_gain);
        } else {
            s += self.previous * feedback * feedback * self.max_gain;
        }
        s = self.input_limiter.process_mono(s);
        if self.soft_limit_gain != 0.0 {
            s *= self.soft_limit_gain;
            s = soft_clip(s);
            s = input + (s - input) * feedback;
        }
        s
    }

    /// Filter the played signal into the next sample's feedback.
    #[inline]
    pub fn feed_back(&mut self, played: f32) {
        let (_, hp) = self.dc_blocker.process(played);
        self.previous += self.lp_coefficient * (hp - self.previous);
    }
}
