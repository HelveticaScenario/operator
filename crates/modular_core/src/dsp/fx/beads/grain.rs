//! One grain: a read head into the recording buffer with its amplitude
//! envelope, voice-stealing fade and stereo placement.

use super::buffer::{Codec, GrainBuffer};
use super::primitives::{Frame, smooth_step};

/// Length of the zone near the write head where grains fade out.
pub const HEAD_CROSSING_SAFETY_ZONE: i32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
enum Stage {
    Attack,
    Sustain,
    Release,
    #[default]
    Done,
}

impl Stage {
    fn index(self) -> usize {
        self as usize
    }

    fn next(self) -> Stage {
        match self {
            Stage::Attack => Stage::Sustain,
            Stage::Sustain => Stage::Release,
            Stage::Release | Stage::Done => Stage::Done,
        }
    }
}

/// Attack/sustain/release grain envelope whose proportions and curvature
/// follow the `shape` control (square → decay → bell → reverse decay).
#[derive(Clone, Copy, Debug, Default)]
struct GrainEnvelope {
    stage: Stage,
    phase: f32,
    phase_increment: [f32; 4],
    smoothness: f32,
    min_attack: f32,
    min_release: f32,
}

impl GrainEnvelope {
    fn init(&mut self, sample_rate: f32) {
        self.stage = Stage::Done;
        self.phase = 0.0;
        self.phase_increment = [0.5, 0.5, 0.5, 0.0];
        self.smoothness = 0.0;
        self.min_attack = 0.0005 * sample_rate;
        self.min_release = 0.010 * sample_rate;
    }

    fn start(&mut self, duration: f32, shape: f32, fractional_sample: f32) {
        self.stage = Stage::Attack;
        self.phase = 0.0;

        let sustain = (1.0 - 3.0 * shape).max(0.0);
        let mut shape = (shape - 0.333_333_33) * 1.5;
        if shape < 0.0 {
            shape = 0.0;
        } else if shape < 0.5 {
            shape = shape * shape * shape * 4.0;
        }
        let attack = shape;
        let release = (1.0 - attack) * (1.0 - sustain);

        let attack_duration = (duration * attack).max(self.min_attack * (1.0 + 3.0 * sustain));
        let sustain_duration = (duration * sustain).max(2.0);
        let release_duration = (duration * release).max(self.min_release);

        self.phase_increment[Stage::Attack.index()] = 1.0 / attack_duration;
        self.phase_increment[Stage::Sustain.index()] = 1.0 / sustain_duration;
        self.phase_increment[Stage::Release.index()] = 1.0 / release_duration;
        self.smoothness = if attack < 0.5 {
            2.0 * attack
        } else {
            2.0 - 2.0 * attack
        };

        self.phase += self.phase_increment[self.stage.index()] * fractional_sample;
        self.wrap_phase();
    }

    fn stop(&mut self) {
        self.stage = Stage::Done;
    }

    fn wrap_phase(&mut self) {
        if self.phase >= 1.0 {
            let t = (self.phase - 1.0) / self.phase_increment[self.stage.index()];
            self.stage = self.stage.next();
            self.phase = t * self.phase_increment[self.stage.index()];
        }
    }

    fn render(&mut self) -> f32 {
        if self.stage == Stage::Done {
            return 0.0;
        }
        let value = if self.stage == Stage::Sustain {
            1.0
        } else {
            let attack = self.stage == Stage::Attack;
            let t = if attack { self.phase } else { 1.0 - self.phase };
            let base = if attack { t } else { (t * t) * (t * t) };
            base + (smooth_step(t) - base) * self.smoothness
        };
        self.phase += self.phase_increment[self.stage.index()];
        self.wrap_phase();
        value
    }

    fn done(&self) -> bool {
        self.stage == Stage::Done
    }
}

/// Linear fade a grain runs when it is about to be stolen for a new one.
#[derive(Clone, Copy, Debug, Default)]
struct VoiceStealingEnvelope {
    value: f32,
    decrement: f32,
}

impl VoiceStealingEnvelope {
    fn reset_and_pause(&mut self) {
        self.value = 1.0;
        self.decrement = 0.0;
    }

    fn start(&mut self, duration: f32) -> i32 {
        self.decrement = 1.0 / duration;
        (self.value * duration) as i32
    }

    fn render(&mut self) -> f32 {
        self.value -= self.decrement;
        self.value.max(0.0)
    }

    fn done(&self) -> bool {
        self.value <= 0.0
    }
}

/// Fractional bits of a grain's position and speed.
const FRACTION_BITS: u32 = 32;
/// One sample in grain position units.
const ONE: i64 = 1 << FRACTION_BITS;

/// Start values for a grain, already mapped to hardware units.
pub struct GrainStart {
    /// Read position behind the write head, in samples.
    pub delay: f32,
    /// Playback rate; negative plays backwards.
    pub speed: f32,
    /// Length in samples.
    pub duration: f32,
    pub shape: f32,
    pub gain: f32,
    /// 0 = hard left, 0.5 = source image unchanged, 1 = hard right.
    pub pan: f32,
    /// Samples elapsed since the trigger onset (0..1).
    pub fractional_sample: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Grain {
    remaining_samples: i32,
    envelope: GrainEnvelope,
    voice_stealing_envelope: VoiceStealingEnvelope,
    /// `gain[destination][source]`.
    gain: [[f32; 2]; 2],
    /// Level before panning, for the feedback path.
    level: f32,
    /// Read position behind the write head and its change per sample, in
    /// 32.32 fixed point.
    delay: i64,
    speed: i64,
}

impl Grain {
    pub fn init(&mut self, sample_rate: f32) {
        self.delay = 0;
        self.speed = ONE;
        self.gain = [[0.5; 2]; 2];
        self.level = 0.0;
        self.envelope.init(sample_rate);
        self.voice_stealing_envelope.reset_and_pause();
        self.remaining_samples = 0;
    }

    #[inline]
    pub fn done(&self) -> bool {
        self.envelope.done() || self.voice_stealing_envelope.done()
    }

    #[inline]
    pub fn remaining_samples(&self) -> i32 {
        self.remaining_samples
    }

    pub fn fade_out(&mut self, duration: i32) {
        if self.remaining_samples > duration {
            self.remaining_samples = self.voice_stealing_envelope.start(duration as f32);
        }
    }

    pub fn start(&mut self, buffer: &GrainBuffer, s: GrainStart) {
        let max_delay = buffer.max_delay() as f32;
        let consumption_speed = s.speed - if buffer.frozen() { 0.0 } else { 1.0 };
        let mut delay = s.delay - s.fractional_sample * consumption_speed;
        let mut duration = s.duration;

        // Keep the grain inside the recorded span for its whole life.
        if consumption_speed != 0.0 {
            duration = duration.min(max_delay / consumption_speed.abs());
        }
        if consumption_speed > 0.0 {
            delay = delay.max(duration * consumption_speed);
        } else if consumption_speed < 0.0 {
            delay = delay.min(max_delay + duration * consumption_speed);
        }

        self.delay = (delay as f64 * ONE as f64) as i64;
        self.speed = (s.speed as f64 * ONE as f64) as i64;

        // Balance that folds the source image toward one side; roughly equal
        // power via a first-order square-root approximation.
        let gain = s.gain;
        self.level = gain;
        if s.pan < 0.5 {
            let pan = s.pan * 2.0;
            self.gain[0][0] = gain;
            self.gain[1][0] = 0.0;
            self.gain[1][1] = pan * (2.0 - pan) * gain;
            self.gain[0][1] = (1.0 - pan) * (1.0 + pan) * gain;
        } else {
            let pan = (1.0 - s.pan) * 2.0;
            self.gain[1][1] = gain;
            self.gain[0][1] = 0.0;
            self.gain[0][0] = pan * (2.0 - pan) * gain;
            self.gain[1][0] = (1.0 - pan) * (1.0 + pan) * gain;
        }

        self.envelope.start(duration, s.shape, s.fractional_sample);
        self.remaining_samples = duration as i32;
        self.voice_stealing_envelope.reset_and_pause();
    }

    /// Advance this grain and return its next sample before gain and pan, or
    /// `None` once it is done. A grain that reaches either end of the buffer
    /// stops.
    #[inline]
    pub fn next_sample<C: Codec>(&mut self, buffer: &GrainBuffer) -> Option<f32> {
        if self.done() {
            return None;
        }

        self.delay -= self.speed;
        if !buffer.frozen() {
            self.delay += ONE;
        }

        let tail = (buffer.max_delay() as i64) << FRACTION_BITS;
        let fade_zone = (HEAD_CROSSING_SAFETY_ZONE as i64) << FRACTION_BITS;
        if self.delay < 0 || self.delay >= tail {
            self.delay = 0;
            self.envelope.stop();
        }

        let d = if self.delay >= tail / 2 {
            self.delay - tail
        } else {
            self.delay
        }
        .abs();
        let fade_ramp = if d < fade_zone {
            d as f32 / fade_zone as f32
        } else {
            1.0
        };

        let delay_integral = (self.delay >> FRACTION_BITS) as i32;
        let delay_fractional = (self.delay & (ONE - 1)) as f32 * (1.0 / ONE as f32);
        let mut sample = buffer.read_hermite::<C>(delay_integral, delay_fractional);
        sample *= smooth_step(fade_ramp);
        sample *= self.envelope.render() * self.voice_stealing_envelope.render();

        self.remaining_samples -= 1;
        if self.done() {
            self.remaining_samples = 0;
        }
        Some(sample)
    }

    /// Mix `sample`, read from a source placed at `place` (left and right
    /// gains), into `out`, and add its unpanned level to `mono`.
    #[inline]
    pub fn mix(&self, sample: f32, place: [f32; 2], out: &mut Frame, mono: &mut f32) {
        let (l, r) = (sample * place[0], sample * place[1]);
        out.l += l * self.gain[0][0] + r * self.gain[0][1];
        out.r += l * self.gain[1][0] + r * self.gain[1][1];
        *mono += sample * self.level;
    }
}
