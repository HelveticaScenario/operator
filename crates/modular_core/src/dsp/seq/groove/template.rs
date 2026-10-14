//! Groove templates: a periodic, monotonic time map over a grid.
//!
//! A template places grid step `k` at `k + offset(k)` steps of straight time.
//! Between those breakpoints time is interpolated linearly.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    MonoSignal,
    dsp::utils::{GATE_HIGH_VOLTAGE, rng::hash_bipolar},
    poly::PolySignal,
    types::Signal,
};

/// Smallest allowed distance between consecutive breakpoints, in grid steps.
/// Keeps the map strictly increasing so the grooved playhead never stalls or
/// runs backwards.
const MIN_STEP_GAP: f64 = 0.01;

const MAX_TEMPLATE_STEPS: usize = 256;
const MAX_GRID: f64 = 192.0;

/// Added before flooring a position into a step index so a position the
/// consumer reads as exactly on a step boundary is never attributed to the
/// step before it.
const STEP_EPSILON: f64 = 1e-9;

fn default_grid() -> f64 {
    16.0
}

fn default_one() -> f64 {
    1.0
}

/// Groove template: per-step timing offsets and velocities over a grid.
#[derive(Clone, Debug, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
pub struct GrooveTemplate {
    /// per-step timing offset in grid steps (positive = late); the template
    /// repeats every `timing.length` steps
    pub timing: Vec<f64>,
    /// per-step velocity (0–1), one per `timing` entry; every step is 1 when empty
    #[serde(default)]
    #[deserr(default)]
    pub velocity: Vec<f64>,
    /// grid steps per cycle (16 = sixteenth notes in a 4/4 bar)
    #[serde(default = "default_grid")]
    #[deserr(default = default_grid())]
    pub grid: f64,
    /// timing strength: scales every offset (0 = straight, 1 = as written)
    #[serde(default = "default_one")]
    #[deserr(default = default_one())]
    pub amount: f64,
    /// velocity strength (0 = every step full, 1 = as written)
    #[serde(default = "default_one")]
    #[deserr(default = default_one())]
    pub velocity_amount: f64,
    /// largest random timing offset per step, in grid steps
    #[serde(default)]
    #[deserr(default)]
    pub random: f64,
    /// seed for the per-step random offsets
    #[serde(default)]
    #[deserr(default)]
    pub seed: u32,
}

impl GrooveTemplate {
    /// Straight sixteenths: no timing offsets, every step at full velocity.
    pub fn straight() -> Self {
        Self {
            timing: vec![0.0],
            velocity: Vec::new(),
            grid: default_grid(),
            amount: 1.0,
            velocity_amount: 1.0,
            random: 0.0,
            seed: 0,
        }
    }

    /// Check that the template is well-formed and that its time map is
    /// strictly increasing for every possible random draw.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.timing.len();
        if n == 0 {
            return Err("timing must have at least one step".into());
        }
        if n > MAX_TEMPLATE_STEPS {
            return Err(format!(
                "timing must have {MAX_TEMPLATE_STEPS} steps or fewer"
            ));
        }
        if !self.grid.is_finite() || self.grid <= 0.0 || self.grid > MAX_GRID {
            return Err(format!(
                "grid must be greater than 0 and at most {MAX_GRID}"
            ));
        }
        if !self.amount.is_finite() || self.amount < 0.0 {
            return Err("amount must be 0 or greater".into());
        }
        if !self.velocity_amount.is_finite() || self.velocity_amount < 0.0 {
            return Err("velocityAmount must be 0 or greater".into());
        }
        if !self.random.is_finite() || self.random < 0.0 {
            return Err("random must be 0 or greater".into());
        }
        if !self.velocity.is_empty() && self.velocity.len() != n {
            return Err(format!(
                "velocity must have one entry per timing step ({n}), got {}",
                self.velocity.len()
            ));
        }
        if self
            .velocity
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0)
        {
            return Err("velocity values must be between 0 and 1".into());
        }
        for (k, t) in self.timing.iter().enumerate() {
            if !t.is_finite() {
                return Err(format!("timing[{k}] must be finite"));
            }
            if (self.amount * t).abs() + self.random >= 1.0 {
                return Err(format!(
                    "timing[{k}] × amount plus random must stay under one grid step"
                ));
            }
        }
        for k in 0..n {
            let next = self.timing[(k + 1) % n];
            let gap = 1.0 + self.amount * (next - self.timing[k]) - 2.0 * self.random;
            if gap < MIN_STEP_GAP {
                return Err(format!(
                    "step {} would land on or before step {k}; reduce the offsets, amount or random",
                    (k + 1) % n
                ));
            }
        }
        Ok(())
    }

    /// Breakpoint offset of absolute step `k`, in grid steps.
    #[inline]
    fn offset(&self, k: i64) -> f64 {
        let n = self.timing.len() as i64;
        let base = self.amount * self.timing[k.rem_euclid(n) as usize];
        if self.random == 0.0 {
            base
        } else {
            // The salt keeps seed 0 from hashing step 0 to a fixed point.
            base + self.random * hash_bipolar(u64::from(self.seed) << 32 | 0x5EED, k as u64)
        }
    }

    /// Map a straight position to its grooved position, both in cycles.
    pub fn warp(&self, straight: f64) -> f64 {
        let s = straight * self.grid;
        // Every |offset| is under one step, so the breakpoint segment holding
        // `s` starts within one step of `floor(s)`: each loop runs at most twice.
        let mut k = s.floor() as i64;
        let mut t_k = k as f64 + self.offset(k);
        while t_k > s {
            k -= 1;
            t_k = k as f64 + self.offset(k);
        }
        let mut t_next = (k + 1) as f64 + self.offset(k + 1);
        while t_next <= s {
            k += 1;
            t_k = t_next;
            t_next = (k + 1) as f64 + self.offset(k + 1);
        }
        (k as f64 + (s - t_k) / (t_next - t_k)) / self.grid
    }

    /// Output velocity (0–5 V) of grooved step `step`.
    pub fn velocity_at_step(&self, step: i64) -> f32 {
        let v = if self.velocity.is_empty() {
            1.0
        } else {
            self.velocity[step.rem_euclid(self.velocity.len() as i64) as usize]
        };
        let scaled = 1.0 - self.velocity_amount * (1.0 - v);
        (scaled.clamp(0.0, 1.0) * 5.0) as f32
    }

    /// Step index of a grooved position, in cycles.
    pub(super) fn step_of(&self, pos: f64) -> i64 {
        (pos * self.grid + STEP_EPSILON).floor() as i64
    }
}

/// Fill a sequencer's unset `groove` and `velocity` inputs before default
/// connections apply. The global groove only lines up with the global
/// playhead, and its velocity only with its offset, so a sequencer given its
/// own playhead or groove plays straight and at full velocity unless given a
/// match.
pub fn fill_groove_inputs(
    playhead: &Option<MonoSignal>,
    groove: &mut Option<MonoSignal>,
    velocity: &mut Option<MonoSignal>,
) {
    let constant = |v: f32| Some(MonoSignal::from_poly(PolySignal::mono(Signal::Volts(v))));
    if playhead.is_some() && groove.is_none() {
        *groove = constant(0.0);
    }
    if (playhead.is_some() || groove.is_some()) && velocity.is_none() {
        *velocity = constant(GATE_HIGH_VOLTAGE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swing(offset: f64) -> GrooveTemplate {
        GrooveTemplate {
            timing: vec![0.0, offset],
            ..GrooveTemplate::straight()
        }
    }

    #[test]
    fn straight_template_is_identity() {
        let t = GrooveTemplate::straight();
        for i in 0..1000 {
            let x = i as f64 * 0.00731;
            assert!((t.warp(x) - x).abs() < 1e-12);
        }
    }

    #[test]
    fn swung_offbeat_lands_late() {
        let t = swing(0.5);
        let step = 1.0 / 16.0;
        // Straight time 1.5 steps is where the groove places the off-beat sixteenth.
        assert!((t.warp(1.5 * step) - step).abs() < 1e-12);
        // Downbeats stay put.
        assert!((t.warp(2.0 * step) - 2.0 * step).abs() < 1e-12);
        // Before the shifted off-beat, the first step is still playing.
        assert!(t.warp(1.25 * step) < step);
    }

    #[test]
    fn warp_is_monotonic_with_random() {
        let t = GrooveTemplate {
            timing: vec![0.0, 0.4, -0.1, 0.2],
            random: 0.2,
            seed: 7,
            ..GrooveTemplate::straight()
        };
        t.validate().unwrap();
        let mut prev = f64::NEG_INFINITY;
        for i in 0..200_000 {
            let x = i as f64 * 1e-5;
            let y = t.warp(x);
            assert!(y > prev, "warp not increasing at {x}");
            prev = y;
        }
    }

    #[test]
    fn validate_rejects_crossing_steps() {
        let t = GrooveTemplate {
            timing: vec![0.9, -0.9],
            ..GrooveTemplate::straight()
        };
        assert!(t.validate().is_err());
        let t = GrooveTemplate {
            velocity: vec![1.0],
            ..swing(0.2)
        };
        assert!(t.validate().is_err());
    }

    #[test]
    fn velocity_follows_step_and_amount() {
        let mut t = GrooveTemplate {
            velocity: vec![1.0, 0.5],
            ..swing(0.0)
        };
        assert_eq!(t.velocity_at_step(0), 5.0);
        assert_eq!(t.velocity_at_step(3), 2.5);
        t.velocity_amount = 0.0;
        assert_eq!(t.velocity_at_step(3), 5.0);
    }
}
