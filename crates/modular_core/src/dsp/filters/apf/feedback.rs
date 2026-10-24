//! The loop feeding a channel's output back into its input.

use crate::dsp::{
    filters::surge::fastmath::fasttanh_clamped,
    utils::{changed, rng::LcgRng, sanitize},
};

use super::stage::{Stage, StageCoeffs};

/// `feedback` at which the loop gain reaches 1 and rings never decay: the
/// end of the control's range.
const ENDLESS_FEEDBACK: f32 = 5.0;

/// Largest `feedback` accepted, past the control's range, where the loop
/// self-oscillates at `MAX_LOOP_GAIN`.
const MAX_FEEDBACK: f32 = 6.0;

/// Loop gain at `MAX_FEEDBACK`.
pub(super) const MAX_LOOP_GAIN: f32 = 1.25;

/// Loss in dB per pass through the loop the curve starts from at `feedback`
/// 0, before it is rescaled to start at a gain of exactly 0.
const SILENT_LOSS_DB: f32 = 60.0;

/// Factor the loss per pass shrinks by for each step of 1 in `feedback`.
const LOSS_RATIO: f32 = 0.25;

/// Least gain the input is scaled by as feedback rises.
pub(super) const MIN_INPUT_GAIN: f32 = 0.3;

/// Level in volts the fed-back signal soft-clips toward.
const FEEDBACK_LIMIT: f32 = 5.0;

/// Peak level in volts of the noise that seeds self-oscillation, about
/// −120 dB below the soft clip.
const NOISE_FLOOR: f32 = 5e-6;

/// Damping `1 / Q` of the bandpass carrying loop gain past 1: about an
/// octave wide.
const EXCESS_DAMPING: f32 = 1.0;

/// Loop gain for `feedback`, signed like it. Up to `ENDLESS_FEEDBACK` the
/// loss per pass shrinks by `LOSS_RATIO` per step, less a constant that
/// brings it to 0, a gain of 1, at `ENDLESS_FEEDBACK`, with the gain rescaled
/// to be exactly 0 at 0; beyond, it rises linearly to `MAX_LOOP_GAIN` at
/// `MAX_FEEDBACK`.
pub(super) fn loop_gain(feedback: f32) -> f32 {
    let amount = feedback.abs().min(MAX_FEEDBACK);
    let gain = if amount <= ENDLESS_FEEDBACK {
        let end = LOSS_RATIO.powf(ENDLESS_FEEDBACK);
        let loss_db = SILENT_LOSS_DB * (LOSS_RATIO.powf(amount) - end) / (1.0 - end);
        let floor = 10f32.powf(-SILENT_LOSS_DB / 20.0);
        (10f32.powf(-loss_db / 20.0) - floor) / (1.0 - floor)
    } else {
        let past = (amount - ENDLESS_FEEDBACK) / (MAX_FEEDBACK - ENDLESS_FEEDBACK);
        1.0 + past * (MAX_LOOP_GAIN - 1.0)
    };
    gain.copysign(feedback)
}

/// Gain on the input at loop gain `gain`. The chain is allpass, so
/// `sqrt(1 - gain²)` holds the loop's average power gain at unity; it stops
/// at `MIN_INPUT_GAIN` so the input stays audible as the loop nears 1.
pub(super) fn input_gain(gain: f32) -> f32 {
    (1.0 - gain * gain)
        .max(MIN_INPUT_GAIN * MIN_INPUT_GAIN)
        .sqrt()
}

/// Loop gain up to 1 feeds the whole output back. Gain past 1 passes only
/// through a bandpass around the cutoff, so self-oscillation builds there
/// rather than on the chain's short-delay modes toward Nyquist.
pub(super) struct FeedbackLoop {
    /// `feedback` the gains were computed for.
    feedback: f32,
    gain: f32,
    /// `gain` limited to ±1: the part fed back whole.
    unity: f32,
    input_gain: f32,
    excess: Stage,
    excess_coeffs: StageCoeffs,
    /// Noise floor in the bandpass, which lets self-oscillation start from
    /// silence.
    noise: LcgRng,
    /// The chain's last output.
    last: f32,
}

impl Default for FeedbackLoop {
    fn default() -> Self {
        Self {
            feedback: f32::NAN,
            gain: 0.0,
            unity: 0.0,
            input_gain: 1.0,
            excess: Stage::default(),
            excess_coeffs: StageCoeffs::default(),
            noise: LcgRng::default(),
            last: 0.0,
        }
    }
}

impl FeedbackLoop {
    pub(super) fn seed_noise(&mut self, base: u64, channel: usize) {
        self.noise.seed(base, channel);
    }

    /// Center the excess bandpass on prewarp `g`.
    pub(super) fn tune(&mut self, g: f32) {
        self.excess_coeffs = StageCoeffs::new(g, EXCESS_DAMPING);
    }

    /// The chain's input: `input`, turned down as feedback rises, plus the
    /// last output fed back at `feedback`.
    #[inline(always)]
    pub(super) fn feed(&mut self, input: f32, feedback: f32) -> f32 {
        if changed(feedback, self.feedback) {
            self.feedback = feedback;
            self.gain = loop_gain(feedback);
            self.unity = self.gain.clamp(-1.0, 1.0);
            self.input_gain = input_gain(self.unity);
        }
        let returned = FEEDBACK_LIMIT * fasttanh_clamped(self.last / FEEDBACK_LIMIT);
        let mut fed = self.unity * returned;
        if self.gain.abs() > 1.0 {
            let floor = NOISE_FLOOR * self.noise.next_bipolar();
            let excess = self.excess.bandpass(&self.excess_coeffs, returned + floor);
            fed += (self.gain - self.unity) * excess;
        } else {
            self.excess = Stage::default();
        }
        sanitize(input * self.input_gain + fed)
    }

    /// Record the chain's output `out`, returning it sanitized.
    #[inline(always)]
    pub(super) fn close(&mut self, out: f32) -> f32 {
        self.last = sanitize(out);
        self.last
    }
}
