# Glicol-style `$plate` and `$dattorro` Rename Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename existing `$plate` to `$dattorro`, create a `OnePole` shared primitive, then build a new `$plate` module implementing the Glicol plate reverb topology.

**Architecture:** The existing Dattorro plate reverb is renamed but otherwise unchanged. A new `OnePole` filter primitive is added to `dsp/utils/`. The new `$plate` module implements the Glicol feedback network as monolithic sample-by-sample DSP with ~20 delay lines, 10 allpass filters, 3 one-pole filters, and a cross-coupled output tap matrix.

**Tech Stack:** Rust (modular_core crate), N-API native build, TypeScript type generation.

**Spec:** `docs/superpowers/specs/2026-04-15-plate-glicol-and-dattorro-rename-design.md`

---

### Task 1: Rename `$plate` → `$dattorro`

**Files:**

- Rename: `crates/modular_core/src/dsp/fx/plate.rs` → `crates/modular_core/src/dsp/fx/dattorro.rs`
- Modify: `crates/modular_core/src/dsp/fx/mod.rs`
- Modify: `crates/modular_core/tests/dsp_fresh_tests.rs:296`

- [ ] **Step 1: Rename the file**

```bash
mv crates/modular_core/src/dsp/fx/plate.rs crates/modular_core/src/dsp/fx/dattorro.rs
```

- [ ] **Step 2: Rename structs inside `dattorro.rs`**

In `crates/modular_core/src/dsp/fx/dattorro.rs`:

- Change the module doc comment from "Dattorro plate reverb module" to "Dattorro reverb module"
- Rename `PlateParams` → `DattorroParams` (all occurrences)
- Rename `PlateOutputs` → `DattorroOutputs` (all occurrences)
- Rename `PlateState` → `DattorroState` (all occurrences)
- Rename `Plate` → `Dattorro` in the struct definition and all `impl` blocks
- Change `#[module(name = "$plate", ...)]` → `#[module(name = "$dattorro", ...)]`
- In the doc comment example, change `$plate(...)` → `$dattorro(...)`

In the test module:

- Rename `make_plate` → `make_dattorro`
- Rename `plate_params` → `dattorro_params`
- Change all `deserializers.get("$plate")` → `deserializers.get("$dattorro")`
- Change all `constructors.get("$plate")` → `constructors.get("$dattorro")`
- Update variable names `plate`, `plate_low`, `plate_high`, `plate_no_mod`, `plate_with_mod` to `dattorro`, `dattorro_low`, `dattorro_high`, `dattorro_no_mod`, `dattorro_with_mod`

- [ ] **Step 3: Update `fx/mod.rs`**

In `crates/modular_core/src/dsp/fx/mod.rs`, change:

- `pub mod plate;` → `pub mod dattorro;`
- `plate::Plate::install_constructor(map);` → `dattorro::Dattorro::install_constructor(map);`
- `plate::Plate::install_params_deserializer(map);` → `dattorro::Dattorro::install_params_deserializer(map);`
- `plate::Plate::get_schema()` → `dattorro::Dattorro::get_schema()`

- [ ] **Step 4: Update integration test**

In `crates/modular_core/tests/dsp_fresh_tests.rs` line 296, change:

```rust
"$plate" => json!({ "input": 0.0 }),
```

to:

```rust
"$dattorro" => json!({ "input": 0.0 }),
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p modular_core -- dattorro
cargo test -p modular_core --test dsp_fresh_tests
```

Expected: All 8 dattorro unit tests pass. All 27 integration tests pass (including `schema_names_match_constructors` and `all_constructors_produce_valid_modules` which validate that `$dattorro` is properly registered).

- [ ] **Step 6: Commit**

```bash
git add -A && git -c commit.gpgsign=false commit -m "refactor: rename \$plate to \$dattorro

The existing module is a faithful Dattorro paper implementation.
Renaming it frees the \$plate name for a new Glicol-inspired
reverb with a denser, more complex topology."
```

---

### Task 2: Add `OnePole` shared primitive

**Files:**

- Create: `crates/modular_core/src/dsp/utils/one_pole.rs`
- Modify: `crates/modular_core/src/dsp/utils/mod.rs:1`

- [ ] **Step 1: Create `one_pole.rs` with tests**

Create `crates/modular_core/src/dsp/utils/one_pole.rs`:

```rust
//! One-pole lowpass filter primitive.
//!
//! A simple first-order IIR filter: `y[n] = x[n] * coeff + y[n-1] * (1 - coeff)`.
//! Higher `coeff` values pass more high-frequency content.

/// A one-pole (first-order) lowpass filter.
///
/// The coefficient controls the cutoff: values near 1.0 pass almost
/// everything, values near 0.0 heavily lowpass the signal.
#[derive(Clone, Debug)]
pub struct OnePole {
    coeff: f32,
    state: f32,
}

impl Default for OnePole {
    fn default() -> Self {
        Self {
            coeff: 0.5,
            state: 0.0,
        }
    }
}

impl OnePole {
    /// Create a new one-pole filter with the given coefficient (0..1).
    pub fn new(coeff: f32) -> Self {
        Self { coeff, state: 0.0 }
    }

    /// Update the filter coefficient.
    #[inline]
    pub fn set_coeff(&mut self, coeff: f32) {
        self.coeff = coeff;
    }

    /// Process one sample through the filter.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        self.state = input * self.coeff + self.state * (1.0 - self.coeff);
        self.state
    }

    /// Reset the filter state to zero.
    pub fn reset(&mut self) {
        self.state = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_half_coeff() {
        let f = OnePole::default();
        assert_eq!(f.coeff, 0.5);
        assert_eq!(f.state, 0.0);
    }

    #[test]
    fn new_sets_coeff() {
        let f = OnePole::new(0.7);
        assert_eq!(f.coeff, 0.7);
    }

    #[test]
    fn coeff_one_passes_through() {
        let mut f = OnePole::new(1.0);
        assert_eq!(f.process(1.0), 1.0);
        assert_eq!(f.process(0.5), 0.5);
        assert_eq!(f.process(0.0), 0.0);
    }

    #[test]
    fn coeff_zero_blocks_signal() {
        let mut f = OnePole::new(0.0);
        // With coeff=0, output is always previous state (starts at 0)
        assert_eq!(f.process(1.0), 0.0);
        assert_eq!(f.process(1.0), 0.0);
    }

    #[test]
    fn lowpass_behavior() {
        // A step input should ramp up gradually with coeff < 1
        let mut f = OnePole::new(0.1);
        let mut prev = 0.0;
        for _ in 0..100 {
            let out = f.process(1.0);
            assert!(out >= prev, "output should monotonically increase");
            prev = out;
        }
        // After many samples, should approach 1.0
        assert!(prev > 0.99, "should converge to input, got {prev}");
    }

    #[test]
    fn reset_clears_state() {
        let mut f = OnePole::new(0.5);
        f.process(1.0);
        assert!(f.state != 0.0);
        f.reset();
        assert_eq!(f.state, 0.0);
    }

    #[test]
    fn set_coeff_changes_behavior() {
        let mut f = OnePole::new(0.1);
        let slow = f.process(1.0);
        f.reset();
        f.set_coeff(0.9);
        let fast = f.process(1.0);
        assert!(fast > slow, "higher coeff should respond faster");
    }
}
```

- [ ] **Step 2: Export from `utils/mod.rs`**

Add `pub mod one_pole;` to the top of `crates/modular_core/src/dsp/utils/mod.rs`, after the existing `pub mod delay_line;` line.

- [ ] **Step 3: Run tests**

```bash
cargo test -p modular_core -- one_pole
```

Expected: All 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add -A && git -c commit.gpgsign=false commit -m "feat: add OnePole lowpass filter primitive to shared utils

Simple first-order IIR filter for use in reverb bandwidth and
damping. Equation: y[n] = x[n] * coeff + y[n-1] * (1 - coeff)."
```

---

### Task 3: Create the new `$plate` module (Glicol topology)

**Files:**

- Create: `crates/modular_core/src/dsp/fx/plate.rs`
- Modify: `crates/modular_core/src/dsp/fx/mod.rs`
- Modify: `crates/modular_core/tests/dsp_fresh_tests.rs`

This is the largest task. The signal flow is documented in the spec. Key reference: `crates/modular_core/src/dsp/fx/dattorro.rs` for the module pattern.

- [ ] **Step 1: Create `plate.rs` with params, state, and skeleton**

Create `crates/modular_core/src/dsp/fx/plate.rs`. The full file structure:

````rust
//! Glicol-style plate reverb module.
//!
//! Implements a dense plate reverberator inspired by the Glicol audio
//! language's plate topology. Uses a longer, more complex feedback network
//! than the standard Dattorro algorithm, producing a thicker, warmer
//! reverb tail with distributed damping and two modulated allpasses.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::delay_line::DelayLine;
use crate::dsp::utils::map_range;
use crate::dsp::utils::one_pole::OnePole;
use crate::poly::{MonoSignal, MonoSignalExt, PolyOutput, PolySignal};
use crate::types::Clickless;

// ─── Reference sample rate ───────────────────────────────────────────────────
// Glicol's DelayN values are in samples at 48kHz.
// Glicol's AllPassFilterGain/DelayMs values are in milliseconds.
const REF_SAMPLE_RATE: f32 = 48000.0;

// ─── Input section delays (milliseconds) ─────────────────────────────────────
const PREDELAY_MS: f32 = 50.0;

// Input diffusion allpasses: (delay_ms, gain)
const INPUT_AP_1: (f32, f32) = (4.771, 0.75);
const INPUT_AP_2: (f32, f32) = (3.595, 0.75);
const INPUT_AP_3: (f32, f32) = (12.72, 0.625);
const INPUT_AP_4: (f32, f32) = (9.307, 0.625);

// ─── Modulated allpasses (milliseconds) ──────────────────────────────────────
const MOD_AP_1_MS: f32 = 100.0; // wet8: gain 0.7
const MOD_AP_2_MS: f32 = 100.0; // bc: gain 0.5

// Modulation excursion: ±5.5 samples at 48kHz when input is ±5V
const REF_MOD_EXCURSION: f32 = 5.5;

// ─── Line A: fixed delay lengths (samples at 48kHz) ─────────────────────────
const LINE_A_1: usize = 394;
const LINE_A_2: usize = 2800;
const LINE_A_3: usize = 1204;

// ─── Line B: delay (samples) + allpasses (ms) ───────────────────────────────
const LINE_B_DELAY: usize = 2000;
const LINE_B_AP_1: (f32, f32) = (7.596, 0.5);
const LINE_B_AP_2: (f32, f32) = (35.78, 0.5);

// ─── Line C: fixed delay lengths (samples at 48kHz) ─────────────────────────
const LINE_C_1: usize = 179;
const LINE_C_2: usize = 2679;
const LINE_C_3: usize = 3500;

// ─── Line D: allpass (ms) + delays (samples) ────────────────────────────────
const LINE_D_AP: (f32, f32) = (30.0, 0.7);
const LINE_D_1: usize = 522;
const LINE_D_2: usize = 2400;
const LINE_D_3: usize = 2400;

// ─── Line E: allpasses (ms) ─────────────────────────────────────────────────
const LINE_E_AP_1: (f32, f32) = (6.2, 0.7);
const LINE_E_AP_2: (f32, f32) = (34.92, 0.7);

// ─── Line F: allpass (ms) + delays (samples) ────────────────────────────────
const LINE_F_AP: (f32, f32) = (20.4, 0.7);
const LINE_F_1: usize = 1578;
const LINE_F_2: usize = 2378;
const LINE_F_3: usize = 2500;

// ─── Helper: convert ms to samples ──────────────────────────────────────────
#[inline]
fn ms_to_samples(ms: f32, sample_rate: f32) -> usize {
    (ms * sample_rate / 1000.0).round() as usize
}

/// Scale a reference sample count (at 48kHz) to the actual sample rate.
#[inline]
fn scale_samples(ref_samples: usize, sample_rate: f32) -> usize {
    ((ref_samples as f32) * sample_rate / REF_SAMPLE_RATE).round() as usize
}

// ─── Params ──────────────────────────────────────────────────────────────────

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct PlateParams {
    /// audio input (even channels → left, odd channels → right)
    input: PolySignal,
    /// input bandwidth — controls high-frequency content entering the tank
    #[signal(default = 1.67, range = (-5.0, 5.0))]
    #[deserr(default)]
    bandwidth: Option<MonoSignal>,
    /// tank damping — higher values absorb more high frequencies per recirculation
    #[signal(default = 4.09, range = (-5.0, 5.0))]
    #[deserr(default)]
    damping: Option<MonoSignal>,
    /// feedback decay — controls how long the reverb tail sustains
    #[signal(default = -2.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    decay: Option<MonoSignal>,
    /// external tank modulation signal
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    modulation: Option<MonoSignal>,
}

// ─── Outputs ─────────────────────────────────────────────────────────────────

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PlateOutputs {
    #[output("output", "stereo reverb output (ch0=left, ch1=right)", default)]
    sample: PolyOutput,
}

// ─── State ───────────────────────────────────────────────────────────────────

#[derive(Default)]
struct PlateState {
    // Input section
    input_lpf: OnePole,
    predelay: DelayLine,
    input_diff: [DelayLine; 4],

    // Feedback junction
    feedback: f32,

    // Modulated allpasses
    mod_ap_1: DelayLine, // wet8: 100ms, gain 0.7
    mod_ap_2: DelayLine, // bc: 100ms, gain 0.5

    // Line A: 3 delay lines
    line_a: [DelayLine; 3],

    // Line B: delay + 2 allpasses
    line_b_delay: DelayLine,
    line_b_lpf: OnePole,
    line_b_ap: [DelayLine; 2],

    // Line C: 3 delay lines
    line_c: [DelayLine; 3],

    // Line D: 1 allpass + 3 delay lines
    line_d_ap: DelayLine,
    line_d_delay: [DelayLine; 3],

    // Line E: 2 allpasses
    line_e_lpf: OnePole,
    line_e_ap: [DelayLine; 2],

    // Line F: 1 allpass + 3 delay lines
    line_f_ap: DelayLine,
    line_f_delay: [DelayLine; 3],

    // Parameter smoothing
    smoothed_bandwidth: Clickless,
    smoothed_damping: Clickless,
    smoothed_decay: Clickless,

    // DC blocking HPF (20Hz)
    dc_prev_in_l: f32,
    dc_prev_in_r: f32,
    dc_prev_out_l: f32,
    dc_prev_out_r: f32,
    dc_block_coeff: f32,

    sample_rate: f32,
}

// ─── Module ──────────────────────────────────────────────────────────────────

/// Stereo plate reverb with a dense Glicol-inspired feedback network.
///
/// Uses a longer feedback path with distributed damping and more allpass
/// stages than the standard Dattorro algorithm, producing a thicker,
/// warmer reverb tail. Always 100% wet — use `.send()` or `$mix` for
/// dry/wet blending.
///
/// ```js
/// $plate($saw('c3')).out()
/// $plate($saw('c3'), { decay: 2, bandwidth: 3 }).out()
/// $plate($saw('c3'), { modulation: $sine('0.1hz') }).out()
/// ```
#[module(name = "$plate", channels = 2, has_init, args(input))]
pub struct Plate {
    outputs: PlateOutputs,
    state: PlateState,
    params: PlateParams,
}

impl Plate {
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;

        // Input section
        self.state.input_lpf = OnePole::new(0.7);
        self.state.predelay = DelayLine::new(ms_to_samples(PREDELAY_MS, sample_rate));

        let input_aps = [INPUT_AP_1, INPUT_AP_2, INPUT_AP_3, INPUT_AP_4];
        for (i, &(ms, _)) in input_aps.iter().enumerate() {
            self.state.input_diff[i] = DelayLine::new(ms_to_samples(ms, sample_rate).max(1));
        }

        // Modulated allpasses — allocate extra for modulation excursion
        let mod_extra = (REF_MOD_EXCURSION * sample_rate / REF_SAMPLE_RATE).ceil() as usize + 2;
        self.state.mod_ap_1 =
            DelayLine::new(ms_to_samples(MOD_AP_1_MS, sample_rate) + mod_extra);
        self.state.mod_ap_2 =
            DelayLine::new(ms_to_samples(MOD_AP_2_MS, sample_rate) + mod_extra);

        // Line A
        self.state.line_a[0] = DelayLine::new(scale_samples(LINE_A_1, sample_rate).max(1));
        self.state.line_a[1] = DelayLine::new(scale_samples(LINE_A_2, sample_rate).max(1));
        self.state.line_a[2] = DelayLine::new(scale_samples(LINE_A_3, sample_rate).max(1));

        // Line B
        self.state.line_b_delay = DelayLine::new(scale_samples(LINE_B_DELAY, sample_rate).max(1));
        self.state.line_b_lpf = OnePole::new(0.1);
        self.state.line_b_ap[0] =
            DelayLine::new(ms_to_samples(LINE_B_AP_1.0, sample_rate).max(1));
        self.state.line_b_ap[1] =
            DelayLine::new(ms_to_samples(LINE_B_AP_2.0, sample_rate).max(1));

        // Line C
        self.state.line_c[0] = DelayLine::new(scale_samples(LINE_C_1, sample_rate).max(1));
        self.state.line_c[1] = DelayLine::new(scale_samples(LINE_C_2, sample_rate).max(1));
        self.state.line_c[2] = DelayLine::new(scale_samples(LINE_C_3, sample_rate).max(1));

        // Line D
        self.state.line_d_ap = DelayLine::new(ms_to_samples(LINE_D_AP.0, sample_rate).max(1));
        self.state.line_d_delay[0] = DelayLine::new(scale_samples(LINE_D_1, sample_rate).max(1));
        self.state.line_d_delay[1] = DelayLine::new(scale_samples(LINE_D_2, sample_rate).max(1));
        self.state.line_d_delay[2] = DelayLine::new(scale_samples(LINE_D_3, sample_rate).max(1));

        // Line E
        self.state.line_e_lpf = OnePole::new(0.1);
        self.state.line_e_ap[0] =
            DelayLine::new(ms_to_samples(LINE_E_AP_1.0, sample_rate).max(1));
        self.state.line_e_ap[1] =
            DelayLine::new(ms_to_samples(LINE_E_AP_2.0, sample_rate).max(1));

        // Line F
        self.state.line_f_ap = DelayLine::new(ms_to_samples(LINE_F_AP.0, sample_rate).max(1));
        self.state.line_f_delay[0] = DelayLine::new(scale_samples(LINE_F_1, sample_rate).max(1));
        self.state.line_f_delay[1] = DelayLine::new(scale_samples(LINE_F_2, sample_rate).max(1));
        self.state.line_f_delay[2] = DelayLine::new(scale_samples(LINE_F_3, sample_rate).max(1));

        // DC blocking coefficient: one-pole HPF at ~20 Hz
        let dc_fc = 20.0_f32;
        self.state.dc_block_coeff = 1.0 - (std::f32::consts::TAU * dc_fc / sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let sample_rate = self.state.sample_rate;
        let num_input_channels = self.params.input.channels();

        // ── Read parameters ──────────────────────────────────────────────

        let bw_v = self.params.bandwidth.value_or(1.67);
        let bw_coeff = map_range(bw_v, -5.0, 5.0, 0.1, 0.9999);
        self.state.smoothed_bandwidth.update(bw_coeff);
        let bandwidth = *self.state.smoothed_bandwidth;

        let damp_v = self.params.damping.value_or(4.09);
        let damp_coeff = map_range(damp_v, -5.0, 5.0, 0.9999, 0.01);
        self.state.smoothed_damping.update(damp_coeff);
        let damping = *self.state.smoothed_damping;

        let decay_v = self.params.decay.value_or(-2.0);
        let decay_raw = map_range(decay_v, -5.0, 5.0, 0.0, 0.9999);
        self.state.smoothed_decay.update(decay_raw);
        let decay = *self.state.smoothed_decay;

        let mod_v = self.params.modulation.value_or(0.0);
        let mod_excursion = mod_v * (REF_MOD_EXCURSION * sample_rate / REF_SAMPLE_RATE) / 5.0;

        // ── Sum input channels to mono ───────────────────────────────────

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
        let mono_in = (left_in + right_in) * 0.5;

        // ── Input bandwidth filter ───────────────────────────────────────

        self.state.input_lpf.set_coeff(bandwidth);
        let filtered = self.state.input_lpf.process(mono_in);

        // ── Predelay (fixed 50ms) ────────────────────────────────────────

        self.state.predelay.write(filtered);
        let predelay_len = ms_to_samples(PREDELAY_MS, sample_rate);
        let predelayed = self.state.predelay.read(predelay_len);

        // ── Input diffusion (4 cascaded allpasses, fixed gains) ──────────

        let input_aps = [INPUT_AP_1, INPUT_AP_2, INPUT_AP_3, INPUT_AP_4];
        let mut diffused = predelayed;
        for (i, &(ms, gain)) in input_aps.iter().enumerate() {
            let delay = ms_to_samples(ms, sample_rate);
            diffused = self.state.input_diff[i].allpass(diffused, delay, gain);
        }

        // ── Feedback junction ────────────────────────────────────────────

        let tank_in = diffused + self.state.feedback;

        // ── Modulated allpass 1 (wet8: 100ms base, gain 0.7) ────────────

        let mod_ap_1_base = ms_to_samples(MOD_AP_1_MS, sample_rate) as f32;
        let mod_ap_1_delay = (mod_ap_1_base + mod_excursion).max(1.0);
        let after_mod_ap_1 =
            self.state
                .mod_ap_1
                .allpass_linear(tank_in, mod_ap_1_delay, 0.7);

        // ── Line A ──────────────────────────────────────────────────────

        let line_a_lens = [LINE_A_1, LINE_A_2, LINE_A_3];
        let mut signal = after_mod_ap_1;
        for (i, &ref_len) in line_a_lens.iter().enumerate() {
            let len = scale_samples(ref_len, sample_rate).max(1);
            self.state.line_a[i].write(signal);
            signal = self.state.line_a[i].read(len);
        }
        // Tap points from Line A (read from end of each delay)
        let tap_aa = self.state.line_a[0].read(scale_samples(LINE_A_1, sample_rate).max(1));
        let tap_ab = self.state.line_a[1].read(scale_samples(LINE_A_2, sample_rate).max(1));
        let after_line_a = signal; // = output of line_a[2]

        // ── Line B ──────────────────────────────────────────────────────

        let line_b_len = scale_samples(LINE_B_DELAY, sample_rate).max(1);
        self.state.line_b_delay.write(after_line_a);
        let after_b_delay = self.state.line_b_delay.read(line_b_len);

        self.state.line_b_lpf.set_coeff(damping);
        let after_b_lpf = self.state.line_b_lpf.process(after_b_delay);

        let after_b_ap1 = self.state.line_b_ap[0].allpass(
            after_b_lpf,
            ms_to_samples(LINE_B_AP_1.0, sample_rate).max(1),
            LINE_B_AP_1.1,
        );
        let tap_ba3 = after_b_ap1; // tap from ba3

        let after_b_ap2 = self.state.line_b_ap[1].allpass(
            after_b_ap1,
            ms_to_samples(LINE_B_AP_2.0, sample_rate).max(1),
            LINE_B_AP_2.1,
        );
        let tap_bb = after_b_ap2;

        // ── Modulated allpass 2 (bc: 100ms base, gain 0.5) ──────────────

        let mod_ap_2_base = ms_to_samples(MOD_AP_2_MS, sample_rate) as f32;
        let mod_ap_2_delay = (mod_ap_2_base + mod_excursion).max(1.0);
        let after_mod_ap_2 =
            self.state
                .mod_ap_2
                .allpass_linear(after_b_ap2, mod_ap_2_delay, 0.5);

        // ── Line C ──────────────────────────────────────────────────────

        let line_c_lens = [LINE_C_1, LINE_C_2, LINE_C_3];
        let mut signal = after_mod_ap_2;
        for (i, &ref_len) in line_c_lens.iter().enumerate() {
            let len = scale_samples(ref_len, sample_rate).max(1);
            self.state.line_c[i].write(signal);
            signal = self.state.line_c[i].read(len);
        }
        let tap_ca = self.state.line_c[0].read(scale_samples(LINE_C_1, sample_rate).max(1));
        let tap_cb = self.state.line_c[1].read(scale_samples(LINE_C_2, sample_rate).max(1));
        let after_line_c = signal * decay; // Mul(decay)

        // ── Line D ──────────────────────────────────────────────────────

        let after_d_ap = self.state.line_d_ap.allpass(
            after_line_c,
            ms_to_samples(LINE_D_AP.0, sample_rate).max(1),
            LINE_D_AP.1,
        );

        let line_d_lens = [LINE_D_1, LINE_D_2, LINE_D_3];
        let mut signal = after_d_ap;
        for (i, &ref_len) in line_d_lens.iter().enumerate() {
            let len = scale_samples(ref_len, sample_rate).max(1);
            self.state.line_d_delay[i].write(signal);
            signal = self.state.line_d_delay[i].read(len);
        }
        let tap_da2 =
            self.state.line_d_delay[0].read(scale_samples(LINE_D_1, sample_rate).max(1));
        let tap_db =
            self.state.line_d_delay[1].read(scale_samples(LINE_D_2, sample_rate).max(1));
        let after_line_d = signal; // = output of line_d_delay[2]

        // ── Line E ──────────────────────────────────────────────────────

        self.state.line_e_lpf.set_coeff(damping);
        let after_e_lpf = self.state.line_e_lpf.process(after_line_d);

        let after_e_ap1 = self.state.line_e_ap[0].allpass(
            after_e_lpf,
            ms_to_samples(LINE_E_AP_1.0, sample_rate).max(1),
            LINE_E_AP_1.1,
        );
        let tap_ea2 = after_e_ap1;

        let after_e_ap2 = self.state.line_e_ap[1].allpass(
            after_e_ap1,
            ms_to_samples(LINE_E_AP_2.0, sample_rate).max(1),
            LINE_E_AP_2.1,
        );
        let tap_eb = after_e_ap2;

        // ── Line F ──────────────────────────────────────────────────────

        let after_f_ap = self.state.line_f_ap.allpass(
            after_e_ap2,
            ms_to_samples(LINE_F_AP.0, sample_rate).max(1),
            LINE_F_AP.1,
        );

        let line_f_lens = [LINE_F_1, LINE_F_2, LINE_F_3];
        let mut signal = after_f_ap;
        for (i, &ref_len) in line_f_lens.iter().enumerate() {
            let len = scale_samples(ref_len, sample_rate).max(1);
            self.state.line_f_delay[i].write(signal);
            signal = self.state.line_f_delay[i].read(len);
        }
        let tap_fa2 =
            self.state.line_f_delay[0].read(scale_samples(LINE_F_1, sample_rate).max(1));
        let tap_fb =
            self.state.line_f_delay[1].read(scale_samples(LINE_F_2, sample_rate).max(1));

        // Feedback: end of Line F * decay → back to input
        self.state.feedback = signal * decay;

        // ── Output tap matrix (matches Glicol) ──────────────────────────

        let left_out = tap_aa + tap_ab + tap_cb - (tap_bb + tap_db + tap_ea2 + tap_fa2);
        let right_out = tap_da2 + tap_db + tap_fb - (tap_eb + tap_ab + tap_ba3 + tap_ca);

        // ── DC blocking HPF ─────────────────────────────────────────────

        let coeff = self.state.dc_block_coeff;

        let dc_out_l = left_out - self.state.dc_prev_in_l + coeff * self.state.dc_prev_out_l;
        self.state.dc_prev_in_l = left_out;
        self.state.dc_prev_out_l = dc_out_l;

        let dc_out_r = right_out - self.state.dc_prev_in_r + coeff * self.state.dc_prev_out_r;
        self.state.dc_prev_in_r = right_out;
        self.state.dc_prev_out_r = dc_out_r;

        // ── Write outputs ────────────────────────────────────────────────

        self.outputs.sample.set(0, dc_out_l);
        self.outputs.sample.set(1, dc_out_r);
    }
}
````

- [ ] **Step 2: Register in `fx/mod.rs`**

In `crates/modular_core/src/dsp/fx/mod.rs`, add `pub mod plate;` after `pub mod dattorro;`, and register in all three functions:

```rust
pub mod dattorro;
pub mod plate;
```

```rust
// In install_constructors:
plate::Plate::install_constructor(map);

// In install_params_deserializers:
plate::Plate::install_params_deserializer(map);

// In schemas:
plate::Plate::get_schema(),
```

- [ ] **Step 3: Add `$plate` to integration test minimal params**

In `crates/modular_core/tests/dsp_fresh_tests.rs`, add after the `$dattorro` entry:

```rust
"$plate" => json!({ "input": 0.0 }),
```

- [ ] **Step 4: Run all tests**

```bash
cargo test -p modular_core -- plate
cargo test -p modular_core --test dsp_fresh_tests
```

Expected: Module compiles, integration tests pass (constructor, schema, tick tests).

- [ ] **Step 5: Commit**

```bash
git add -A && git -c commit.gpgsign=false commit -m "feat: add new \$plate module with Glicol-inspired reverb topology

Dense feedback network with distributed damping, 10 allpass filters,
and ~20 delay lines. Thicker, warmer tail than \$dattorro. Params:
bandwidth (input filtering), damping (tank HF absorption), decay
(feedback gain), modulation (external tank modulation)."
```

---

### Task 4: Add unit tests for new `$plate`

**Files:**

- Modify: `crates/modular_core/src/dsp/fx/plate.rs` (append test module)

- [ ] **Step 1: Add test module to `plate.rs`**

Append the following test module to the end of `crates/modular_core/src/dsp/fx/plate.rs`:

```rust
#[cfg(test)]
mod tests {
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
        constructors.get("$plate").unwrap()(&"test-plate".to_string(), SAMPLE_RATE, deserialized)
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

    fn plate_params(overrides: serde_json::Value) -> serde_json::Value {
        let mut base = json!({ "input": 0.0 });
        if let (Some(base_map), Some(over_map)) = (base.as_object_mut(), overrides.as_object()) {
            for (k, v) in over_map {
                base_map.insert(k.clone(), v.clone());
            }
        }
        base
    }

    #[test]
    fn works_with_only_input() {
        let plate = make_plate(json!({ "input": 1.0 }));
        let (left, right) = collect_stereo(&**plate, 10000);
        let left_energy: f32 = left.iter().map(|s| s * s).sum();
        let right_energy: f32 = right.iter().map(|s| s * s).sum();
        assert!(left_energy > 0.0, "should produce output with default params");
        assert!(right_energy > 0.0, "should produce output with default params");
    }

    #[test]
    fn silence_in_silence_out() {
        let plate = make_plate(plate_params(json!({})));
        let (left, right) = collect_stereo(&**plate, 1000);
        assert!(left.iter().all(|&s| s == 0.0), "left should be silent");
        assert!(right.iter().all(|&s| s == 0.0), "right should be silent");
    }

    #[test]
    fn impulse_produces_output() {
        let plate = make_plate(plate_params(json!({ "input": 1.0, "decay": 3.0 })));
        let (left, right) = collect_stereo(&**plate, 20000);
        let left_energy: f32 = left.iter().map(|s| s * s).sum();
        let right_energy: f32 = right.iter().map(|s| s * s).sum();
        assert!(left_energy > 0.0, "left channel should have energy from impulse");
        assert!(right_energy > 0.0, "right channel should have energy from impulse");
    }

    #[test]
    fn stereo_channels_differ() {
        let plate = make_plate(plate_params(json!({ "input": 1.0, "decay": 3.0 })));
        let (left, right) = collect_stereo(&**plate, 10000);
        let identical = left.iter().zip(right.iter()).all(|(l, r)| (l - r).abs() < 1e-10);
        assert!(!identical, "left and right channels should be decorrelated");
    }

    #[test]
    fn no_dc_offset_accumulation() {
        let plate = make_plate(plate_params(json!({ "input": 1.0, "decay": 2.0 })));
        let (left, right) = collect_stereo(&**plate, 48000);
        let last_left = &left[47000..];
        let last_right = &right[47000..];
        let left_mean: f32 = last_left.iter().sum::<f32>() / last_left.len() as f32;
        let right_mean: f32 = last_right.iter().sum::<f32>() / last_right.len() as f32;
        assert!(left_mean.abs() < 10.0, "left DC offset should be bounded, got: {left_mean}");
        assert!(right_mean.abs() < 10.0, "right DC offset should be bounded, got: {right_mean}");
    }

    #[test]
    fn higher_decay_produces_longer_tail() {
        let plate_low = make_plate(plate_params(json!({ "input": 1.0, "decay": -3.0 })));
        let plate_high = make_plate(plate_params(json!({ "input": 1.0, "decay": 3.0 })));
        let n = 20000;
        let (left_low, _) = collect_stereo(&**plate_low, n);
        let (left_high, _) = collect_stereo(&**plate_high, n);
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
        let plate = make_plate(plate_params(json!({})));
        step(&**plate);
        let poly = plate.get_poly_sample(DEFAULT_PORT).unwrap();
        assert_eq!(poly.channels(), 2, "output should be stereo (2 channels)");
    }

    #[test]
    fn modulation_changes_output() {
        let n = 20000;
        let plate_no_mod = make_plate(plate_params(json!({ "input": 1.0, "decay": 3.0 })));
        let (left_no_mod, _) = collect_stereo(&**plate_no_mod, n);
        let plate_with_mod = make_plate(plate_params(
            json!({ "input": 1.0, "decay": 3.0, "modulation": 2.5 }),
        ));
        let (left_with_mod, _) = collect_stereo(&**plate_with_mod, n);
        let differs = left_no_mod
            .iter()
            .zip(left_with_mod.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(differs, "modulated plate should produce different output than unmodulated");
    }

    #[test]
    fn bandwidth_affects_brightness() {
        // Low bandwidth should produce less high-frequency content
        let n = 10000;
        let plate_bright = make_plate(plate_params(json!({ "input": 1.0, "bandwidth": 5.0 })));
        let plate_dark = make_plate(plate_params(json!({ "input": 1.0, "bandwidth": -5.0 })));
        let (left_bright, _) = collect_stereo(&**plate_bright, n);
        let (left_dark, _) = collect_stereo(&**plate_dark, n);
        // Different bandwidth should produce different output
        let differs = left_bright
            .iter()
            .zip(left_dark.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(differs, "different bandwidth should produce different output");
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p modular_core -- plate::tests
```

Expected: All 9 tests pass.

- [ ] **Step 3: Commit**

```bash
git add -A && git -c commit.gpgsign=false commit -m "test: add unit tests for new \$plate module"
```

---

### Task 5: Build native module and regenerate TypeScript types

**Files:**

- Modified (regenerated): `crates/modular/schemas.json`
- Modified (regenerated): `generated/dsl.d.ts`

- [ ] **Step 1: Build native module**

```bash
yarn build-native
```

Expected: Build succeeds, `schemas.json` regenerated with both `$dattorro` and `$plate` schemas.

- [ ] **Step 2: Regenerate TypeScript types**

```bash
yarn generate-lib
```

- [ ] **Step 3: Run TypeScript typecheck**

```bash
yarn typecheck
```

Expected: Clean typecheck. The generated types should show `$dattorro` and `$plate` as separate modules, both with their respective optional params.

- [ ] **Step 4: Commit**

```bash
git add -f generated/dsl.d.ts && git add crates/modular/schemas.json && git -c commit.gpgsign=false commit -m "build: regenerate schemas and types for \$plate and \$dattorro"
```
