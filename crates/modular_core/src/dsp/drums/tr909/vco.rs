//! The triangle VCO the 909's kick and snare share: an integrator between a
//! comparator's two thresholds.

use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};

/// One triangle VCO: the integrator's voltage and the comparator's state.
#[derive(Clone, Copy, Default)]
pub struct Vco<L: Lanes = Mono> {
    pub tri: L::S,
    /// The comparator's output: rising while true.
    pub rising: L::SM,
}

lanewise!(Vco { tri, rising });

impl<L: Lanes> Vco<L> {
    /// Advances `dt` at slope `slope` V/s, reflecting off the comparator's
    /// thresholds at ±`threshold`.
    #[inline]
    pub fn run(&mut self, slope: L::S, dt: f32, threshold: f32) {
        let mut travel = slope * dt;
        let mut active = travel.gt(c(0.0));
        while active.any() {
            let room = L::S::select(
                self.rising,
                c::<L::S>(threshold) - self.tri,
                self.tri + threshold,
            );
            let finish = active & travel.lt(room);
            let moved = self.tri + L::S::select(self.rising, travel, -travel);
            let reflect = active & !finish;
            let edge = L::S::select(self.rising, c(threshold), c(-threshold));
            self.tri = L::S::select(finish, moved, L::S::select(reflect, edge, self.tri));
            travel = L::S::select(reflect, travel - room.max(c(0.0)), travel);
            self.rising = (reflect & !self.rising) | (!reflect & self.rising);
            active = reflect & travel.gt(c(0.0));
        }
    }

    /// Advances one sample: while `triggered`, the integrator is held
    /// toward zero with `coeff` per sample and the comparator low, so the
    /// triangle starts downward; otherwise it runs at `slope`.
    #[inline]
    pub fn advance(&mut self, triggered: L::SM, slope: L::S, dt: f32, coeff: f32, threshold: f32) {
        let held = Self {
            tri: self.tri * coeff,
            rising: L::SM::default(),
        };
        let mut run = *self;
        if !triggered.all() {
            run.run(slope, dt, threshold);
        }
        *self = Self {
            tri: L::S::select(triggered, held.tri, run.tri),
            rising: (triggered & held.rising) | (!triggered & run.rising),
        };
    }
}
