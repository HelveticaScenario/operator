//! Runtime state that turns a straight playhead into groove outputs.

use super::template::GrooveTemplate;
use crate::dsp::seq::FRESH_WINDOW_SECS;
use crate::dsp::utils::{TempGate, TempGateState, min_gate_samples};

/// Keeps a grooved position from running backwards while straight time moves
/// forward, so a template edit or a change of groove waits for the new map to
/// catch up instead of replaying notes already played. Backward straight moves
/// (transport reset, Link) pass through.
#[derive(Clone, Copy)]
pub struct GrooveHold {
    prev_straight: f64,
    prev_grooved: f64,
}

impl Default for GrooveHold {
    fn default() -> Self {
        Self {
            prev_straight: f64::NEG_INFINITY,
            prev_grooved: f64::NEG_INFINITY,
        }
    }
}

impl GrooveHold {
    /// The grooved position to use for this sample.
    pub fn apply(&mut self, straight: f64, grooved: f64) -> f64 {
        let grooved = if straight >= self.prev_straight {
            grooved.max(self.prev_grooved)
        } else {
            grooved
        };
        self.prev_straight = straight;
        self.prev_grooved = grooved;
        grooved
    }
}

/// True while straight time has started but a late first step has not: the
/// grooved position is still below zero, so nothing plays yet.
pub fn before_first_step(straight: f64, grooved: f64) -> bool {
    straight >= 0.0 && grooved < 0.0
}

/// One sample of groove output.
pub struct GrooveFrame {
    /// Grooved minus straight position, in cycles.
    pub offset: f32,
    /// Velocity of the current grooved step, 0–5 V.
    pub velocity: f32,
    /// Trigger at the start of each grooved step, 0 or 5 V.
    pub trigger: f32,
}

/// What the next step decides about its trigger.
#[derive(Clone, Copy)]
enum TriggerSync {
    /// A fresh follower's first sample: record the step and its position.
    Fresh,
    /// A fresh follower's second sample: the position's speed reveals how long
    /// before creation the recorded step began, and the trigger fires one
    /// sample late if that is within [`FRESH_WINDOW_SECS`].
    Probing {
        /// Position in grid steps on the first sample.
        steps: f64,
    },
    /// The transport just started: fire the first step reached.
    Restart,
    /// Fire on forward step changes.
    Tracking,
}

/// Runtime state that turns a straight playhead into grooved outputs.
pub struct GrooveFollower {
    hold: GrooveHold,
    sync: TriggerSync,
    /// Most recent step passed.
    step: i64,
    trigger: TempGate,
}

impl Default for GrooveFollower {
    fn default() -> Self {
        Self {
            hold: GrooveHold::default(),
            sync: TriggerSync::Fresh,
            step: 0,
            trigger: TempGate::new_gate(TempGateState::Low),
        }
    }
}

impl GrooveFollower {
    /// Fire the trigger on the first step reached after a transport start.
    pub fn restart(&mut self) {
        self.sync = TriggerSync::Restart;
    }

    /// Advance to straight position `straight` (cycles), exactly as the
    /// sequencers reading the offset see it: the playhead's channels summed
    /// in f64.
    pub fn advance(
        &mut self,
        template: &GrooveTemplate,
        straight: f64,
        sample_rate: f32,
    ) -> GrooveFrame {
        let grooved = self.hold.apply(straight, template.warp(straight));
        // Index the step from the sum sequencers compute, so velocity and the
        // trigger change on exactly the sample a grid-aligned note fires.
        let offset = (grooved - straight) as f32;
        let position = straight + offset as f64;
        let step = template.step_of(position.max(0.0));
        if !before_first_step(straight, position)
            && self.reach(step, position * template.grid, sample_rate)
        {
            self.trigger.set_state(
                TempGateState::High,
                TempGateState::Low,
                min_gate_samples(sample_rate),
            );
        }

        GrooveFrame {
            offset,
            velocity: template.velocity_at_step(step),
            trigger: self.trigger.process(),
        }
    }

    /// Record reaching `step` (at `steps` grid steps) and return whether its
    /// trigger fires.
    fn reach(&mut self, step: i64, steps: f64, sample_rate: f32) -> bool {
        let last = self.step;
        let tracking = matches!(
            self.sync,
            TriggerSync::Tracking | TriggerSync::Probing { .. }
        );
        let fire = match self.sync {
            TriggerSync::Fresh => false,
            TriggerSync::Restart => true,
            TriggerSync::Probing { steps: first } if step == last => {
                let per_sample = steps - first;
                let window = per_sample * FRESH_WINDOW_SECS * sample_rate as f64;
                per_sample > 0.0 && first - last as f64 <= window
            }
            TriggerSync::Probing { .. } | TriggerSync::Tracking => step > last,
        };
        // A rewind of one step is jitter (a Link resync) and keeps the previous
        // step so it does not fire again; a longer rewind is a seek and
        // re-anchors silently.
        let jitter = tracking && step < last && last - step <= 1;
        if !jitter {
            self.step = step;
        }
        self.sync = match self.sync {
            TriggerSync::Fresh => TriggerSync::Probing { steps },
            _ => TriggerSync::Tracking,
        };
        fire
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
    fn follower_triggers_once_per_grooved_step() {
        let t = swing(0.5);
        let mut f = GrooveFollower::default();
        let sr = 48_000.0;
        // One cycle at 0.5 cycles per second.
        let samples = 96_000;
        let mut rising = 0;
        let mut was_high = false;
        let mut trigger_samples = Vec::new();
        for i in 0..samples {
            let frame = f.advance(&t, i as f64 / samples as f64, sr);
            let high = frame.trigger > 0.0;
            if high && !was_high {
                rising += 1;
                trigger_samples.push(i);
            }
            was_high = high;
        }
        assert_eq!(rising, 16);
        // Off-beat triggers are delayed by half a sixteenth.
        let sixteenth = samples / 16;
        assert_eq!(trigger_samples[1], sixteenth + sixteenth / 2);
    }

    #[test]
    fn follower_holds_through_backward_template_edit() {
        let late = swing(0.5);
        let mut f = GrooveFollower::default();
        let sr = 48_000.0;
        let x = 1.9 / 16.0;
        let before = f.advance(&GrooveTemplate::straight(), x, sr);
        let after = f.advance(&late, x + 1e-6, sr);
        assert!(x + 1e-6 + after.offset as f64 >= x + before.offset as f64);
    }

    /// One sixteenth every 1000 samples.
    const SAMPLES_PER_STEP: f64 = 1000.0;

    /// Samples where the trigger rises, with the straight position given in
    /// samples by `at`.
    fn triggers(t: &GrooveTemplate, f: &mut GrooveFollower, at: &[f64]) -> Vec<usize> {
        let mut out = Vec::new();
        let mut was_high = false;
        for (i, s) in at.iter().enumerate() {
            let straight = s / SAMPLES_PER_STEP / 16.0;
            let high = f.advance(t, straight, 48_000.0).trigger > 0.0;
            if high && !was_high {
                out.push(i);
            }
            was_high = high;
        }
        out
    }

    fn ramp(from: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| from + i as f64).collect()
    }

    #[test]
    fn fresh_follower_mid_step_waits_for_the_next_step() {
        let t = GrooveTemplate::straight();
        let mut f = GrooveFollower::default();
        let fired = triggers(&t, &mut f, &ramp(3370.0, 2000));
        assert_eq!(fired, vec![630, 1630]);
    }

    #[test]
    fn fresh_follower_on_a_step_boundary_fires_it() {
        let t = GrooveTemplate::straight();
        let mut f = GrooveFollower::default();
        let fired = triggers(&t, &mut f, &ramp(3000.0, 1500));
        assert_eq!(fired, vec![1, 1000]);
    }

    #[test]
    fn backward_jump_rearms_without_firing() {
        let t = GrooveTemplate::straight();
        let mut f = GrooveFollower::default();
        let mut at = ramp(0.0, 20_500);
        at.extend(ramp(500.0, 1000));
        let fired = triggers(&t, &mut f, &at);
        let after_jump: Vec<usize> = fired.into_iter().filter(|&i| i >= 20_500).collect();
        assert_eq!(after_jump, vec![21_000]);
    }

    #[test]
    fn one_step_rewind_does_not_fire_again() {
        let t = GrooveTemplate::straight();
        let mut f = GrooveFollower::default();
        let mut at = ramp(0.0, 5100);
        at.extend(ramp(4900.0, 1200));
        let fired = triggers(&t, &mut f, &at);
        assert_eq!(fired, vec![1, 1000, 2000, 3000, 4000, 5000, 6200]);
    }

    #[test]
    fn restart_fires_a_late_first_step_when_it_arrives() {
        let t = GrooveTemplate {
            timing: vec![0.3, 0.0],
            ..GrooveTemplate::straight()
        };
        let mut f = GrooveFollower::default();
        f.restart();
        let fired = triggers(&t, &mut f, &ramp(0.0, 1500));
        assert_eq!(fired.len(), 2, "{fired:?}");
        assert!(fired[0].abs_diff(300) <= 1, "{fired:?}");
        assert_eq!(fired[1], 1000);
    }
}
