# `$plate` Dattorro Reverb Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `$plate` stereo Dattorro plate reverb module to the `fx` category, with shared delay line primitives in `modular_core::dsp::utils`.

**Architecture:** Convert `utils.rs` into a `utils/` directory module, add a reusable `DelayLine` struct, then build the Dattorro reverb as a standard `#[module]` with `has_init` for delay line allocation. The module sums even input channels to left, odd to right, runs a single stereo Dattorro tank, and outputs 2-channel stereo.

**Tech Stack:** Rust (modular_core crate), proc macros (#[module], Outputs, Connect, etc.)

---

### Task 1: Convert `utils.rs` to `utils/` directory module

**Files:**

- Move: `crates/modular_core/src/dsp/utils.rs` → `crates/modular_core/src/dsp/utils/mod.rs`

- [ ] **Step 1: Move the file**

```bash
mkdir -p crates/modular_core/src/dsp/utils
mv crates/modular_core/src/dsp/utils.rs crates/modular_core/src/dsp/utils/mod.rs
```

- [ ] **Step 2: Verify existing tests still pass**

Run: `cargo test -p modular_core`
Expected: All tests pass — `pub mod utils;` in `dsp/mod.rs` will now resolve to `utils/mod.rs` instead of `utils.rs`. No code changes needed.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "refactor: convert dsp::utils from file to directory module"
```

---

### Task 2: Add `DelayLine` shared primitive

**Files:**

- Create: `crates/modular_core/src/dsp/utils/delay_line.rs`
- Modify: `crates/modular_core/src/dsp/utils/mod.rs` (add `pub mod delay_line;`)

- [ ] **Step 1: Write the delay_line module with tests**

Create `crates/modular_core/src/dsp/utils/delay_line.rs`:

````rust
//! Reusable fixed-capacity delay line for audio DSP.
//!
//! Buffer is power-of-2 sized for efficient wrapping via bitwise AND.
//! All memory is allocated at construction time — no heap allocation
//! during audio processing.

/// A fixed-capacity delay line backed by a `Vec<f32>`.
///
/// The buffer is sized to the next power of 2 >= `max_delay + 1`,
/// enabling wrapping via bitwise AND instead of modulo.
pub struct DelayLine {
    buffer: Vec<f32>,
    write_ptr: usize,
    mask: usize,
}

impl Default for DelayLine {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            write_ptr: 0,
            mask: 0,
        }
    }
}

impl DelayLine {
    /// Create a new delay line that can hold up to `max_delay` samples.
    ///
    /// The internal buffer is rounded up to the next power of 2 for
    /// efficient wrapping. Panics if `max_delay` is 0.
    pub fn new(max_delay: usize) -> Self {
        assert!(max_delay > 0, "max_delay must be > 0");
        let size = (max_delay + 1).next_power_of_two();
        Self {
            buffer: vec![0.0; size],
            write_ptr: 0,
            mask: size - 1,
        }
    }

    /// Write a sample at the current write position and advance the pointer.
    #[inline]
    pub fn write(&mut self, sample: f32) {
        self.buffer[self.write_ptr & self.mask] = sample;
        self.write_ptr = self.write_ptr.wrapping_add(1);
    }

    /// Read a sample at `delay` samples behind the current write position.
    ///
    /// A delay of 0 reads the most recently written sample.
    #[inline]
    pub fn read(&self, delay: usize) -> f32 {
        self.buffer[self.write_ptr.wrapping_sub(1).wrapping_sub(delay) & self.mask]
    }

    /// Read with linear interpolation at a fractional delay.
    #[inline]
    pub fn read_linear(&self, delay: f32) -> f32 {
        let delay_int = delay as usize;
        let frac = delay - delay_int as f32;
        let a = self.read(delay_int);
        let b = self.read(delay_int + 1);
        a + frac * (b - a)
    }

    /// Process one sample through an allpass filter embedded in this delay line.
    ///
    /// Writes the input (mixed with feedback), reads from the delay tap,
    /// and returns the allpass output. The standard allpass topology:
    ///
    /// ```text
    ///   input ──┬──[× coeff]──(+)── write to delay
    ///           │               ↑
    ///           │    delay_read─┘
    ///           │        │
    ///           (+)──[× -coeff]── output
    ///            ↑
    ///     delay_read
    /// ```
    #[inline]
    pub fn allpass(&mut self, input: f32, delay: usize, coefficient: f32) -> f32 {
        let delayed = self.read(delay);
        let write_val = input + coefficient * delayed;
        self.write(write_val);
        delayed - coefficient * write_val
    }

    /// Clear all samples to zero without deallocating.
    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write_ptr = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_power_of_two_buffer() {
        let dl = DelayLine::new(100);
        assert_eq!(dl.buffer.len(), 128); // next power of 2 >= 101
        assert_eq!(dl.mask, 127);
    }

    #[test]
    fn new_exact_power_of_two() {
        let dl = DelayLine::new(127);
        assert_eq!(dl.buffer.len(), 128); // 127+1 = 128, already power of 2
    }

    #[test]
    #[should_panic(expected = "max_delay must be > 0")]
    fn new_zero_panics() {
        DelayLine::new(0);
    }

    #[test]
    fn write_and_read_delay_zero() {
        let mut dl = DelayLine::new(10);
        dl.write(42.0);
        assert_eq!(dl.read(0), 42.0);
    }

    #[test]
    fn write_and_read_various_delays() {
        let mut dl = DelayLine::new(10);
        for i in 0..5 {
            dl.write(i as f32);
        }
        // Most recent write was 4.0
        assert_eq!(dl.read(0), 4.0);
        assert_eq!(dl.read(1), 3.0);
        assert_eq!(dl.read(2), 2.0);
        assert_eq!(dl.read(3), 1.0);
        assert_eq!(dl.read(4), 0.0);
    }

    #[test]
    fn wrapping_works() {
        let mut dl = DelayLine::new(4); // buffer size = 8
        // Write more samples than buffer size
        for i in 0..20 {
            dl.write(i as f32);
        }
        assert_eq!(dl.read(0), 19.0);
        assert_eq!(dl.read(1), 18.0);
    }

    #[test]
    fn read_linear_interpolation() {
        let mut dl = DelayLine::new(10);
        dl.write(0.0);
        dl.write(10.0);
        // delay 0.0 = most recent (10.0)
        assert_eq!(dl.read_linear(0.0), 10.0);
        // delay 1.0 = previous (0.0)
        assert_eq!(dl.read_linear(1.0), 0.0);
        // delay 0.5 = halfway between 10.0 and 0.0
        assert_eq!(dl.read_linear(0.5), 5.0);
        // delay 0.25 = 75% of 10.0 + 25% of 0.0
        assert_eq!(dl.read_linear(0.25), 7.5);
    }

    #[test]
    fn allpass_unity_gain() {
        // Allpass should have unity magnitude response
        let mut dl = DelayLine::new(100);
        let coeff = 0.5;
        let delay = 10;

        // Feed an impulse and collect output energy
        let mut input_energy = 0.0f64;
        let mut output_energy = 0.0f64;

        // Impulse
        let out = dl.allpass(1.0, delay, coeff);
        input_energy += 1.0;
        output_energy += (out as f64) * (out as f64);

        // Collect tail
        for _ in 0..200 {
            let out = dl.allpass(0.0, delay, coeff);
            output_energy += (out as f64) * (out as f64);
        }

        // Energy should be preserved (within floating point tolerance)
        assert!(
            (output_energy - input_energy).abs() < 0.01,
            "allpass energy: input={input_energy}, output={output_energy}"
        );
    }

    #[test]
    fn clear_zeros_buffer() {
        let mut dl = DelayLine::new(10);
        for i in 0..10 {
            dl.write(i as f32 + 1.0);
        }
        dl.clear();
        for i in 0..10 {
            assert_eq!(dl.read(i), 0.0);
        }
    }

    #[test]
    fn default_is_empty() {
        let dl = DelayLine::default();
        assert!(dl.buffer.is_empty());
        assert_eq!(dl.mask, 0);
    }
}
````

- [ ] **Step 2: Add the module declaration to utils/mod.rs**

Add at the top of `crates/modular_core/src/dsp/utils/mod.rs`:

```rust
pub mod delay_line;
```

- [ ] **Step 3: Run tests to verify**

Run: `cargo test -p modular_core delay_line`
Expected: All 9 tests pass.

- [ ] **Step 4: Run full test suite to verify no regressions**

Run: `cargo test -p modular_core`
Expected: All existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: add reusable DelayLine primitive with allpass support"
```

---

### Task 3: Create the `$plate` Dattorro reverb module

**Files:**

- Create: `crates/modular_core/src/dsp/fx/plate.rs`
- Modify: `crates/modular_core/src/dsp/fx/mod.rs`

- [ ] **Step 1: Create the plate module**

Create `crates/modular_core/src/dsp/fx/plate.rs`:

````rust
//! Dattorro plate reverb module.
//!
//! Implements Jon Dattorro's plate reverberator algorithm from
//! "Effect Design Part 1: Reverberator and Other Filters" (JAES, 1997).

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::delay_line::DelayLine;
use crate::dsp::utils::map_range;
use crate::poly::{MonoSignal, MonoSignalExt, PolyOutput, PolySignal, PolySignalExt};

// ─── Dattorro delay lengths (reference sample rate: 29761 Hz) ────────────────

const REF_SAMPLE_RATE: f32 = 29761.0;

// Input diffuser allpass delay lengths
const INPUT_DIFF_1: f32 = 142.0;
const INPUT_DIFF_2: f32 = 107.0;
const INPUT_DIFF_3: f32 = 379.0;
const INPUT_DIFF_4: f32 = 277.0;

// Tank decay diffusion allpass delay lengths
const DECAY_DIFF_1: f32 = 672.0;
const DECAY_DIFF_2: f32 = 908.0;

// Tank delay line lengths
const TANK_DELAY_1: f32 = 4453.0;
const TANK_DELAY_2: f32 = 4217.0;

// Output tap positions (from Dattorro's Table 1)
// Left output taps
const TAP_L1: f32 = 266.0;  // from tank_delay_1
const TAP_L2: f32 = 2974.0; // from tank_delay_1
const TAP_L3: f32 = 1913.0; // from decay_diff_2
const TAP_L4: f32 = 1996.0; // from tank_delay_2
const TAP_L5: f32 = 1990.0; // from tank_delay_2
const TAP_L6: f32 = 187.0;  // from decay_diff_1
const TAP_L7: f32 = 1066.0; // from tank_delay_1

// Right output taps
const TAP_R1: f32 = 353.0;  // from tank_delay_2
const TAP_R2: f32 = 3627.0; // from tank_delay_2
const TAP_R3: f32 = 1228.0; // from decay_diff_1
const TAP_R4: f32 = 2673.0; // from tank_delay_1
const TAP_R5: f32 = 2111.0; // from tank_delay_1
const TAP_R6: f32 = 335.0;  // from decay_diff_2
const TAP_R7: f32 = 121.0;  // from tank_delay_2

// Maximum predelay in seconds
const MAX_PREDELAY_SECS: f32 = 0.5;

/// Scale a reference delay length to the actual sample rate, then multiply
/// by the size factor.
#[inline]
fn scale_delay(ref_samples: f32, sample_rate: f32, size: f32) -> usize {
    ((ref_samples * sample_rate / REF_SAMPLE_RATE) * size).round() as usize
}

// ─── Params ──────────────────────────────────────────────────────────────────

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct PlateParams {
    /// audio input (even channels → left, odd channels → right)
    input: PolySignal,
    /// reverb decay time (-5 to 5)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    decay: MonoSignal,
    /// high-frequency damping in the reverb tank (-5 to 5)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    damping: MonoSignal,
    /// room size — scales all delay line lengths (-5 to 5)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    size: MonoSignal,
    /// predelay time in seconds (0 to 0.5)
    #[signal(default = 0.0, range = (0.0, 0.5))]
    predelay: MonoSignal,
    /// input diffusion amount (0 to 5)
    #[signal(default = 3.5, range = (0.0, 5.0))]
    diffusion: MonoSignal,
}

// ─── Outputs ─────────────────────────────────────────────────────────────────

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PlateOutputs {
    #[output("output", "stereo reverb output (ch0=left, ch1=right)", default)]
    sample: PolyOutput,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Pre-allocated state for the Dattorro reverb tank.
///
/// All `DelayLine`s default to empty and are allocated in `init()`.
#[derive(Default)]
struct PlateState {
    // Predelay
    predelay_l: DelayLine,
    predelay_r: DelayLine,

    // Input diffusers (4 cascaded allpass filters)
    input_diff: [DelayLine; 4],

    // Tank: left path
    decay_diff_l: DelayLine,
    tank_delay_l: DelayLine,
    damp_state_l: f32,

    // Tank: right path
    decay_diff_r: DelayLine,
    tank_delay_r: DelayLine,
    damp_state_r: f32,

    // Cross-feedback
    feedback_l: f32,
    feedback_r: f32,

    // Cached sample rate for parameter mapping
    sample_rate: f32,
}

// ─── Module ──────────────────────────────────────────────────────────────────

/// Stereo plate reverb based on the Dattorro algorithm.
///
/// Implements Jon Dattorro's plate reverberator with input diffusion,
/// a cross-coupled stereo tank, and multi-tap output. Even input
/// channels are summed to the left input, odd channels to the right.
/// Output is always 100% wet.
///
/// ```js
/// $plate($saw('c3'), { decay: 3, damping: 1, size: 2 }).out()
/// ```
#[module(name = "$plate", channels = 2, has_init, args(input))]
pub struct Plate {
    outputs: PlateOutputs,
    state: PlateState,
    params: PlateParams,
}

impl Plate {
    /// Allocate all delay lines based on the sample rate.
    /// Called once at construction time on the main thread.
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;

        // Use a generous size multiplier for allocation so that the size
        // param can scale delay lengths up at runtime without exceeding capacity.
        let max_size = 2.5;

        // Predelay: up to MAX_PREDELAY_SECS
        let max_predelay = (MAX_PREDELAY_SECS * sample_rate).ceil() as usize;
        self.state.predelay_l = DelayLine::new(max_predelay);
        self.state.predelay_r = DelayLine::new(max_predelay);

        // Input diffusers
        let input_diff_lengths = [INPUT_DIFF_1, INPUT_DIFF_2, INPUT_DIFF_3, INPUT_DIFF_4];
        for (i, &ref_len) in input_diff_lengths.iter().enumerate() {
            let max_len = scale_delay(ref_len, sample_rate, max_size);
            self.state.input_diff[i] = DelayLine::new(max_len.max(1));
        }

        // Tank
        self.state.decay_diff_l =
            DelayLine::new(scale_delay(DECAY_DIFF_1, sample_rate, max_size).max(1));
        self.state.tank_delay_l =
            DelayLine::new(scale_delay(TANK_DELAY_1, sample_rate, max_size).max(1));
        self.state.decay_diff_r =
            DelayLine::new(scale_delay(DECAY_DIFF_2, sample_rate, max_size).max(1));
        self.state.tank_delay_r =
            DelayLine::new(scale_delay(TANK_DELAY_2, sample_rate, max_size).max(1));
    }

    fn update(&mut self, _sample_rate: f32) {
        let sample_rate = self.state.sample_rate;
        let num_input_channels = self.params.input.channels();

        // ── Read parameters ──────────────────────────────────────────────

        // Map bipolar -5..5 to algorithm coefficients
        let decay_v = self.params.decay.value();
        let decay_coeff = map_range(decay_v, -5.0, 5.0, 0.0, 0.9995);

        let damp_v = self.params.damping.value();
        // Higher damping voltage = more damping = lower bandwidth
        let bandwidth = map_range(damp_v, -5.0, 5.0, 0.9999, 0.1);

        let size_v = self.params.size.value();
        let size = map_range(size_v, -5.0, 5.0, 0.25, 2.0);

        let predelay_secs = self.params.predelay.value().clamp(0.0, MAX_PREDELAY_SECS);
        let predelay_samples = (predelay_secs * sample_rate) as usize;

        let diff_v = self.params.diffusion.value();
        let input_diff_coeff = map_range(diff_v, 0.0, 5.0, 0.0, 0.75);
        let decay_diff_1_coeff = map_range(diff_v, 0.0, 5.0, 0.0, 0.70);
        let decay_diff_2_coeff = map_range(diff_v, 0.0, 5.0, 0.0, 0.50);

        // ── Sum input channels to stereo ─────────────────────────────────

        let mut left_in = 0.0f32;
        let mut right_in = 0.0f32;
        for ch in 0..num_input_channels {
            let sample = self.params.input.get_value(ch);
            if ch % 2 == 0 {
                left_in += sample;
            } else {
                right_in += sample;
            }
        }

        // ── Predelay ─────────────────────────────────────────────────────

        self.state.predelay_l.write(left_in);
        let left_predelayed = self.state.predelay_l.read(predelay_samples);

        self.state.predelay_r.write(right_in);
        let right_predelayed = self.state.predelay_r.read(predelay_samples);

        // Sum to mono for input diffusers
        let mono_in = (left_predelayed + right_predelayed) * 0.5;

        // ── Input diffusion (4 cascaded allpass filters) ─────────────────

        let diff_delays = [INPUT_DIFF_1, INPUT_DIFF_2, INPUT_DIFF_3, INPUT_DIFF_4];
        let mut diffused = mono_in;
        for (i, &ref_len) in diff_delays.iter().enumerate() {
            let delay = scale_delay(ref_len, sample_rate, size).max(1);
            diffused = self.state.input_diff[i].allpass(diffused, delay, input_diff_coeff);
        }

        // ── Tank processing ──────────────────────────────────────────────

        // Left tank: input = diffused + right feedback
        let left_tank_in = diffused + self.state.feedback_r * decay_coeff;

        // Decay diffusion allpass (left)
        let dd_l_delay = scale_delay(DECAY_DIFF_1, sample_rate, size).max(1);
        let left_after_ap = self.state.decay_diff_l.allpass(
            left_tank_in,
            dd_l_delay,
            -decay_diff_1_coeff,
        );

        // Delay line (left)
        self.state.tank_delay_l.write(left_after_ap);
        let td_l_delay = scale_delay(TANK_DELAY_1, sample_rate, size).max(1);
        let left_tank_out = self.state.tank_delay_l.read(td_l_delay);

        // Damping (one-pole lowpass)
        self.state.damp_state_l =
            left_tank_out * bandwidth + self.state.damp_state_l * (1.0 - bandwidth);
        let left_damped = self.state.damp_state_l;

        // Right tank: input = diffused + left feedback
        let right_tank_in = diffused + self.state.feedback_l * decay_coeff;

        // Decay diffusion allpass (right)
        let dd_r_delay = scale_delay(DECAY_DIFF_2, sample_rate, size).max(1);
        let right_after_ap = self.state.decay_diff_r.allpass(
            right_tank_in,
            dd_r_delay,
            -decay_diff_2_coeff,
        );

        // Delay line (right)
        self.state.tank_delay_r.write(right_after_ap);
        let td_r_delay = scale_delay(TANK_DELAY_2, sample_rate, size).max(1);
        let right_tank_out = self.state.tank_delay_r.read(td_r_delay);

        // Damping (one-pole lowpass)
        self.state.damp_state_r =
            right_tank_out * bandwidth + self.state.damp_state_r * (1.0 - bandwidth);
        let right_damped = self.state.damp_state_r;

        // Store feedback (cross-coupled)
        self.state.feedback_l = left_damped * decay_coeff;
        self.state.feedback_r = right_damped * decay_coeff;

        // ── Output taps ──────────────────────────────────────────────────

        // Scale tap positions
        let tap_l1 = scale_delay(TAP_L1, sample_rate, size);
        let tap_l2 = scale_delay(TAP_L2, sample_rate, size);
        let tap_l3 = scale_delay(TAP_L3, sample_rate, size);
        let tap_l4 = scale_delay(TAP_L4, sample_rate, size);
        let tap_l5 = scale_delay(TAP_L5, sample_rate, size);
        let tap_l6 = scale_delay(TAP_L6, sample_rate, size);
        let tap_l7 = scale_delay(TAP_L7, sample_rate, size);

        let left_out = self.state.tank_delay_l.read(tap_l1)
            + self.state.tank_delay_l.read(tap_l2)
            - self.state.decay_diff_r.read(tap_l3)
            + self.state.tank_delay_r.read(tap_l4)
            - self.state.tank_delay_l.read(tap_l5)
            - self.state.decay_diff_l.read(tap_l6)
            - self.state.tank_delay_l.read(tap_l7);

        let tap_r1 = scale_delay(TAP_R1, sample_rate, size);
        let tap_r2 = scale_delay(TAP_R2, sample_rate, size);
        let tap_r3 = scale_delay(TAP_R3, sample_rate, size);
        let tap_r4 = scale_delay(TAP_R4, sample_rate, size);
        let tap_r5 = scale_delay(TAP_R5, sample_rate, size);
        let tap_r6 = scale_delay(TAP_R6, sample_rate, size);
        let tap_r7 = scale_delay(TAP_R7, sample_rate, size);

        let right_out = self.state.tank_delay_r.read(tap_r1)
            + self.state.tank_delay_r.read(tap_r2)
            - self.state.decay_diff_l.read(tap_r3)
            + self.state.tank_delay_l.read(tap_r4)
            - self.state.tank_delay_r.read(tap_r5)
            - self.state.decay_diff_r.read(tap_r6)
            - self.state.tank_delay_r.read(tap_r7);

        // Scale output (0.6 factor to prevent clipping with dense input)
        let output_gain = 0.6;
        self.outputs.sample.set(0, left_out * output_gain);
        self.outputs.sample.set(1, right_out * output_gain);
    }
}

message_handlers!(impl Plate {});
````

- [ ] **Step 2: Register the module in fx/mod.rs**

Modify `crates/modular_core/src/dsp/fx/mod.rs` — add after `pub mod segment;`:

```rust
pub mod plate;
```

Add to `install_constructors`:

```rust
plate::Plate::install_constructor(map);
```

Add to `install_params_deserializers`:

```rust
plate::Plate::install_params_deserializer(map);
```

Add to `schemas`:

```rust
plate::Plate::get_schema(),
```

The full updated `mod.rs` should be:

```rust
//! Effects (FX) modules category.
//!
//! Contains waveshaping and distortion effects adapted from
//! the 4ms Ensemble Oscillator warp and twist modes.
//! Copyright 4ms Company. Used under GPL v3.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod enosc_tables;

pub mod cheby;
pub mod fold;
pub mod plate;
pub mod segment;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    fold::Fold::install_constructor(map);
    cheby::Cheby::install_constructor(map);
    segment::Segment::install_constructor(map);
    plate::Plate::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    fold::Fold::install_params_deserializer(map);
    cheby::Cheby::install_params_deserializer(map);
    segment::Segment::install_params_deserializer(map);
    plate::Plate::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        fold::Fold::get_schema(),
        cheby::Cheby::get_schema(),
        segment::Segment::get_schema(),
        plate::Plate::get_schema(),
    ]
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p modular_core`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: add $plate Dattorro stereo plate reverb module"
```

---

### Task 4: Add `$plate` to integration test infrastructure

**Files:**

- Modify: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Add `$plate` minimal params to the `minimal_params` function**

In `crates/modular_core/tests/dsp_fresh_tests.rs`, find the `minimal_params` match statement and add a new arm alongside the existing fx modules. Find this line:

```rust
"$cheby" | "$fold" | "$segment" => json!({ "input": 0.0, "amount": 0.0 }),
```

Add after it:

```rust
"$plate" => json!({ "input": 0.0 }),
```

- [ ] **Step 2: Run the integration tests**

Run: `cargo test -p modular_core --test dsp_fresh_tests`
Expected: `all_constructors_produce_valid_modules` and `all_constructors_can_tick` both pass, including the new `$plate` module.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "test: add $plate to integration test infrastructure"
```

---

### Task 5: Add plate module unit tests

**Files:**

- Modify: `crates/modular_core/src/dsp/fx/plate.rs` (add tests at bottom)

- [ ] **Step 1: Add unit tests to the plate module**

Add at the bottom of `crates/modular_core/src/dsp/fx/plate.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::types::Sampleable;
    use serde_json::json;
    use std::sync::Arc;

    const SAMPLE_RATE: f32 = 48000.0;
    const DEFAULT_PORT: &str = "output";

    fn make_plate(params: serde_json::Value) -> Arc<Box<dyn Sampleable>> {
        let constructors = get_constructors();
        let deserializers = get_params_deserializers();
        let deserializer = deserializers.get("$plate").unwrap();
        let cached = deserializer(params).unwrap();
        let deserialized = DeserializedParams {
            params: cached.params,
            argument_spans: Default::default(),
            channel_count: cached.channel_count,
        };
        constructors.get("$plate").unwrap()(
            &"test-plate".to_string(),
            SAMPLE_RATE,
            deserialized,
        )
        .unwrap()
    }

    fn step(module: &dyn Sampleable) {
        module.tick();
        module.update();
    }

    fn collect_stereo(module: &dyn Sampleable, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        for _ in 0..n {
            step(module);
            let poly = module.get_poly_sample(DEFAULT_PORT).unwrap();
            left.push(poly.get(0));
            right.push(poly.get(1));
        }
        (left, right)
    }

    #[test]
    fn silence_in_silence_out() {
        let plate = make_plate(json!({ "input": 0.0 }));
        let (left, right) = collect_stereo(&**plate, 1000);
        assert!(left.iter().all(|&s| s == 0.0), "left should be silent");
        assert!(right.iter().all(|&s| s == 0.0), "right should be silent");
    }

    #[test]
    fn impulse_produces_output() {
        let plate = make_plate(json!({ "input": 1.0, "decay": 3.0 }));
        // Collect enough samples for the reverb tail to develop
        let (left, right) = collect_stereo(&**plate, 10000);

        // After the initial transient, there should be non-zero output
        let left_energy: f32 = left.iter().map(|s| s * s).sum();
        let right_energy: f32 = right.iter().map(|s| s * s).sum();
        assert!(left_energy > 0.0, "left channel should have energy from impulse");
        assert!(right_energy > 0.0, "right channel should have energy from impulse");
    }

    #[test]
    fn stereo_channels_differ() {
        // The Dattorro algorithm produces decorrelated stereo from different tap points
        let plate = make_plate(json!({ "input": 1.0, "decay": 3.0, "size": 2.0 }));
        let (left, right) = collect_stereo(&**plate, 5000);

        // L and R should not be identical (stereo decorrelation)
        let identical = left.iter().zip(right.iter()).all(|(l, r)| (l - r).abs() < 1e-10);
        assert!(!identical, "left and right channels should be decorrelated");
    }

    #[test]
    fn no_dc_offset_accumulation() {
        // Feed constant DC and check that output doesn't grow unbounded
        let plate = make_plate(json!({ "input": 1.0, "decay": 2.0 }));
        let (left, right) = collect_stereo(&**plate, 48000); // 1 second

        // Check the last 1000 samples for DC offset stability
        let last_left = &left[47000..];
        let last_right = &right[47000..];
        let left_mean: f32 = last_left.iter().sum::<f32>() / last_left.len() as f32;
        let right_mean: f32 = last_right.iter().sum::<f32>() / last_right.len() as f32;

        // DC offset should be bounded (not growing)
        assert!(
            left_mean.abs() < 10.0,
            "left DC offset should be bounded, got: {left_mean}"
        );
        assert!(
            right_mean.abs() < 10.0,
            "right DC offset should be bounded, got: {right_mean}"
        );
    }

    #[test]
    fn higher_decay_produces_longer_tail() {
        // Compare energy with low vs high decay
        let plate_low = make_plate(json!({ "input": 1.0, "decay": -3.0 }));
        let plate_high = make_plate(json!({ "input": 1.0, "decay": 3.0 }));

        let n = 20000;
        let (left_low, _) = collect_stereo(&**plate_low, n);
        let (left_high, _) = collect_stereo(&**plate_high, n);

        // Measure energy in the tail (last quarter)
        let tail_start = n * 3 / 4;
        let low_tail_energy: f32 = left_low[tail_start..].iter().map(|s| s * s).sum();
        let high_tail_energy: f32 = left_high[tail_start..].iter().map(|s| s * s).sum();

        assert!(
            high_tail_energy > low_tail_energy,
            "higher decay should have more tail energy: high={high_tail_energy}, low={low_tail_energy}"
        );
    }

    #[test]
    fn output_is_two_channels() {
        let plate = make_plate(json!({ "input": 0.0 }));
        step(&**plate);
        let poly = plate.get_poly_sample(DEFAULT_PORT).unwrap();
        assert_eq!(poly.channels(), 2, "output should be stereo (2 channels)");
    }
}
```

- [ ] **Step 2: Run the unit tests**

Run: `cargo test -p modular_core plate`
Expected: All 6 plate tests pass.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p modular_core`
Expected: All tests pass including the new ones.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "test: add plate reverb unit tests"
```

---

### Task 6: Build native module and run JS tests

**Files:** None to modify — this is a verification step.

- [ ] **Step 1: Build the native module**

Run: `yarn build-native`
Expected: Builds successfully. The `$plate` schema will be included in the generated `schemas.json`.

- [ ] **Step 2: Regenerate TypeScript types**

Run: `yarn generate-lib`
Expected: The generated TypeScript types include `$plate` with its parameters.

- [ ] **Step 3: Run TypeScript type checking**

Run: `yarn typecheck`
Expected: No type errors.

- [ ] **Step 4: Run JS unit tests**

Run: `yarn test:unit`
Expected: All tests pass. The DSL executor tests should recognize the new `$plate` module from the schemas.

- [ ] **Step 5: Commit any generated file changes**

```bash
git add -A && git commit -m "build: regenerate types for $plate module"
```
