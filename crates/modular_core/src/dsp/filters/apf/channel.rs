use std::ops::Range;

use crate::dsp::utils::{changed, sanitize};

use super::{
    MAX_STAGES,
    feedback::FeedbackLoop,
    stage::{Stage, StageCoeffs, damping, glide, prewarp},
};

/// One sample's controls for a channel.
#[derive(Clone, Copy)]
pub(super) struct Controls {
    pub cutoff: f32,
    pub stages: f32,
    pub resonance: f32,
    pub feedback: f32,
}

/// The stage counts of the two chains `stages` crossfades between at a step
/// of `step` stages: 1, `1 + step`, `1 + 2·step`, …, with 50 as the last.
pub(super) fn grid_points(stages: f32, step: usize) -> (usize, usize) {
    let lower = 1 + (stages - 1.0) as usize / step * step;
    (lower, (lower + step).min(MAX_STAGES))
}

/// A channel's chain of stages and its feedback loop.
pub(super) struct AllpassChannel {
    stages: [Stage; MAX_STAGES],
    /// Coefficients of every whole stage, and of the stage gliding in.
    coeffs: StageCoeffs,
    gliding: StageCoeffs,
    /// Prewarp and damping of `coeffs`.
    g: f32,
    k: f32,
    /// Cutoff in octaves relative to the sample rate, resonance and
    /// fractional stage count the coefficients were computed for.
    tuned_octaves: f32,
    tuned_resonance: f32,
    tuned_frac: f32,
    feedback: FeedbackLoop,
    /// Stages run on the last sample; the rest hold stale state.
    active: usize,
    /// Whether the chain has run a sample. Until then every stage starts
    /// from rest; afterwards one coming into use starts settled on its
    /// input, as though it had been passing it steadily. A stage gliding in
    /// sits near Nyquist, where starting from rest would leave it ringing.
    running: bool,
}

impl Default for AllpassChannel {
    fn default() -> Self {
        Self {
            stages: [Stage::default(); MAX_STAGES],
            coeffs: StageCoeffs::default(),
            gliding: StageCoeffs::default(),
            g: 0.0,
            k: 0.0,
            tuned_octaves: f32::NAN,
            tuned_resonance: f32::NAN,
            tuned_frac: f32::NAN,
            feedback: FeedbackLoop::default(),
            active: 0,
            running: false,
        }
    }
}

impl AllpassChannel {
    /// Seed this channel's noise floor from the module's `base` and its
    /// index `channel`, so modules and channels self-oscillate independently.
    pub(super) fn seed_noise(&mut self, base: u64, channel: usize) {
        self.feedback.seed_noise(base, channel);
    }

    /// Recompute whichever coefficients `octaves` (the cutoff relative to the
    /// sample rate), `resonance` and the gliding stage's `frac` change; `frac`
    /// 0 leaves the gliding stage's for later.
    #[inline(always)]
    fn tune(&mut self, octaves: f32, resonance: f32, frac: f32) {
        let new_cutoff = changed(octaves, self.tuned_octaves);
        let new_resonance = changed(resonance, self.tuned_resonance);
        if new_cutoff {
            self.tuned_octaves = octaves;
            self.g = prewarp(octaves);
            self.feedback.tune(self.g);
        }
        if new_resonance {
            self.tuned_resonance = resonance;
            self.k = damping(resonance);
        }
        if new_cutoff || new_resonance {
            self.coeffs = StageCoeffs::new(self.g, self.k);
            self.tuned_frac = f32::NAN;
        }
        if frac > 0.0 && changed(frac, self.tuned_frac) {
            self.tuned_frac = frac;
            self.gliding = StageCoeffs::from_reciprocal(glide(self.g, frac), self.k);
        }
    }

    /// Bring the first `count` stages into use, returning the index from
    /// which they start settled on their input. Before the first sample every
    /// stage is at rest.
    fn activate(&mut self, count: usize) -> usize {
        let before = self.active;
        self.active = count;
        if self.running { before } else { usize::MAX }
    }

    /// Run the whole stages in `range`; those from `settle_from` start
    /// settled on their input.
    #[inline(always)]
    fn run(&mut self, range: Range<usize>, settle_from: usize, mut x: f32) -> f32 {
        let split = settle_from.clamp(range.start, range.end);
        for stage in &mut self.stages[range.start..split] {
            x = stage.allpass(&self.coeffs, x);
        }
        for stage in &mut self.stages[split..range.end] {
            stage.settle(x);
            x = stage.allpass(&self.coeffs, x);
        }
        x
    }

    /// `whole` stages, with the next gliding in by `frac`.
    fn glide_chain(&mut self, x: f32, whole: usize, frac: f32) -> f32 {
        let gliding = frac > 0.0;
        let settle_from = self.activate(whole + usize::from(gliding));
        let x = self.run(0..whole, settle_from, x);
        if !gliding {
            return x;
        }
        let stage = &mut self.stages[whole];
        if whole >= settle_from {
            stage.settle(x);
        }
        stage.allpass(&self.gliding, x)
    }

    /// The chains at the grid points either side of `stages`, blended by
    /// where `stages` falls between them. The upper chain continues from the
    /// lower one.
    fn crossfade_chain(&mut self, x: f32, stages: f32, step: usize) -> f32 {
        let (lower, upper) = grid_points(stages, step);
        let mix = (stages - lower as f32) / (upper - lower).max(1) as f32;
        let settle_from = self.activate(if mix > 0.0 { upper } else { lower });
        let below = self.run(0..lower, settle_from, x);
        if mix == 0.0 {
            return below;
        }
        let above = self.run(lower..upper, settle_from, below);
        below + mix * (above - below)
    }

    /// One sample. `fade_step` 0 glides the next stage in by a fractional
    /// stage count; above 0 it crossfades between chains `fade_step` stages
    /// apart. `log2_c4_over_fs` converts the V/Oct cutoff to octaves
    /// relative to the sample rate.
    #[inline(always)]
    pub(super) fn step(
        &mut self,
        input: f32,
        controls: Controls,
        fade_step: usize,
        log2_c4_over_fs: f32,
    ) -> f32 {
        let stages = sanitize(controls.stages).clamp(1.0, MAX_STAGES as f32);
        let whole = stages as usize;
        let frac = stages - whole as f32;
        self.tune(
            sanitize(controls.cutoff) + log2_c4_over_fs,
            sanitize(controls.resonance),
            if fade_step == 0 { frac } else { 0.0 },
        );
        let x = self.feedback.feed(input, sanitize(controls.feedback));
        let out = if fade_step == 0 {
            self.glide_chain(x, whole, frac)
        } else {
            self.crossfade_chain(x, stages, fade_step)
        };
        if !out.is_finite() {
            self.stages = [Stage::default(); MAX_STAGES];
        }
        self.running = true;
        self.feedback.close(out)
    }
}
