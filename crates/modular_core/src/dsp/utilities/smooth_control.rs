use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::sanitize;

/// Seconds the output takes to reach each new value. Longer than the interval
/// between UI drag events, so consecutive ramps join into one continuous line
/// instead of a staircase.
const RAMP_SECONDS: f32 = 0.03;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct SmoothControlParams {
    /// value the output ramps to
    value: f64,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SmoothControlOutputs {
    #[output("output", "smoothed control value", default)]
    sample: f32,
}

#[derive(Default)]
struct SmoothControlState {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp_samples: u32,
    initialized: bool,
}

/// A UI-driven constant: the editor sets `value` through the single-module
/// update path, and the output ramps linearly to it over [`RAMP_SECONDS`] so
/// a drag's stepped values move without zipper noise. A freshly built control
/// starts at its value; one that replaces another continues from the old
/// output. Backs `$slider`, the out gain/pan faders, and the mute/solo gate.
#[module(name = "_smoothControl", has_init, patch_update, args(value))]
pub struct SmoothControl {
    outputs: SmoothControlOutputs,
    params: SmoothControlParams,
    state: SmoothControlState,
}

impl SmoothControl {
    fn init(&mut self, sample_rate: f32) {
        self.state.ramp_samples = ((RAMP_SECONDS * sample_rate) as u32).max(1);
    }

    fn update(&mut self, _sample_rate: f32) {
        let state = &mut self.state;
        if state.remaining > 0 {
            state.remaining -= 1;
            state.current = if state.remaining == 0 {
                state.target
            } else {
                state.current + state.step
            };
        }
        self.outputs.sample = state.current;
    }
}

impl crate::types::PatchUpdateHandler for SmoothControl {
    fn on_patch_update(&mut self) {
        let value = sanitize(self.params.value as f32);
        let state = &mut self.state;
        if !state.initialized {
            state.current = value;
            state.target = value;
            state.initialized = true;
        } else if value != state.target {
            state.target = value;
            state.step = (value - state.current) / state.ramp_samples as f32;
            state.remaining = state.ramp_samples;
        }
    }
}

message_handlers!(impl SmoothControl {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, PatchUpdateHandler};

    const SR: f32 = 48_000.0;

    fn make(value: f64) -> SmoothControl {
        let mut outputs = SmoothControlOutputs::default();
        outputs.set_all_channels(1);
        let mut s = SmoothControl {
            params: SmoothControlParams { value },
            outputs,
            state: SmoothControlState::default(),
            _channel_count: 1,
            _block_index: Default::default(),
        };
        s.init(SR);
        s.on_patch_update();
        s
    }

    /// Mirrors a single-module update: a new control at `value` takes the old
    /// one's state, then runs its patch-update hook.
    fn replace(old: &mut SmoothControl, value: f64) -> SmoothControl {
        let mut new = make(value);
        new.state = std::mem::take(&mut old.state);
        new.on_patch_update();
        new
    }

    #[test]
    fn starts_at_its_value_without_ramping() {
        let mut s = make(3.0);
        s.update(SR);
        assert_eq!(s.outputs.sample, 3.0);
    }

    #[test]
    fn ramps_linearly_to_a_new_value_and_lands_exactly() {
        let mut s = make(0.0);
        s.update(SR);
        let mut s = replace(&mut s, 1.0);
        let samples = (RAMP_SECONDS * SR) as usize;
        let mut prev = 0.0;
        for _ in 0..samples - 1 {
            s.update(SR);
            let out = s.outputs.sample;
            assert!(out > prev && out < 1.0);
            assert!((out - prev - 1.0 / samples as f32).abs() < 1e-5);
            prev = out;
        }
        s.update(SR);
        assert_eq!(s.outputs.sample, 1.0);
        s.update(SR);
        assert_eq!(s.outputs.sample, 1.0);
    }

    #[test]
    fn an_unchanged_value_does_not_restart_the_ramp() {
        let mut s = make(0.0);
        s.update(SR);
        let mut s = replace(&mut s, 1.0);
        for _ in 0..100 {
            s.update(SR);
        }
        let before = s.state.remaining;
        s.on_patch_update();
        assert_eq!(s.state.remaining, before);
    }

    #[test]
    fn a_new_value_mid_ramp_continues_from_the_current_output() {
        let mut s = make(0.0);
        s.update(SR);
        let mut s = replace(&mut s, 1.0);
        for _ in 0..100 {
            s.update(SR);
        }
        let before = s.outputs.sample;
        let mut s = replace(&mut s, -1.0);
        s.update(SR);
        let max_step = 2.0 / (RAMP_SECONDS * SR);
        assert!(s.outputs.sample < before);
        assert!(before - s.outputs.sample <= max_step);
    }

    #[test]
    fn a_staircase_of_values_produces_no_output_jump_larger_than_one_ramp_step() {
        let mut s = make(0.0);
        s.update(SR);
        let event_interval = (0.016 * SR) as usize;
        let mut prev = s.outputs.sample;
        let mut largest = 0.0f32;
        for stair in 1..=20 {
            s = replace(&mut s, stair as f64 * 0.5);
            for _ in 0..event_interval {
                s.update(SR);
                largest = largest.max((s.outputs.sample - prev).abs());
                prev = s.outputs.sample;
            }
        }
        assert!(largest < 0.5 / (RAMP_SECONDS * SR) * 2.0);
    }
}
