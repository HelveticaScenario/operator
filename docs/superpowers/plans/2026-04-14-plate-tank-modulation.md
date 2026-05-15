# Tank Modulation for `$plate` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an optional `modulation` param to `$plate` that modulates decay diffusion allpass delay lengths via an external signal, enabling chorus/detuning effects in the reverb tank.

**Architecture:** Add `allpass_linear()` to `DelayLine` for fractional-sample allpass reads, add `modulation` param to `PlateParams`, and use the new method for the two decay diffusion stages when modulation is present. All other tank stages remain integer-delay.

**Tech Stack:** Rust (modular_core crate), N-API type generation (yarn build-native + yarn generate-lib)

---

### Task 1: Add `allpass_linear()` method to `DelayLine`

**Files:**

- Modify: `crates/modular_core/src/dsp/utils/delay_line.rs:72-77` (add method after existing `allpass`)
- Test: `crates/modular_core/src/dsp/utils/delay_line.rs` (add test in `mod tests`)

- [ ] **Step 1: Write the failing test for `allpass_linear` energy preservation**

Add this test at the end of the `mod tests` block in `crates/modular_core/src/dsp/utils/delay_line.rs`, after the `default_is_empty` test (line 203):

```rust
    #[test]
    fn allpass_linear_unity_gain() {
        // allpass_linear should preserve energy just like allpass
        let mut dl = DelayLine::new(100);
        let coeff = 0.5;
        let delay = 10.5_f32; // fractional delay

        let mut input_energy = 0.0f64;
        let mut output_energy = 0.0f64;

        // Impulse
        let out = dl.allpass_linear(1.0, delay, coeff);
        input_energy += 1.0;
        output_energy += (out as f64) * (out as f64);

        // Collect tail
        for _ in 0..200 {
            let out = dl.allpass_linear(0.0, delay, coeff);
            output_energy += (out as f64) * (out as f64);
        }

        assert!(
            (output_energy - input_energy).abs() < 0.01,
            "allpass_linear energy: input={input_energy}, output={output_energy}"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular_core allpass_linear_unity_gain`
Expected: FAIL — `allpass_linear` method does not exist yet.

- [ ] **Step 3: Implement `allpass_linear` on `DelayLine`**

Add this method in `crates/modular_core/src/dsp/utils/delay_line.rs`, directly after the existing `allpass` method (after line 77):

```rust
    /// Process one sample through an allpass filter with fractional-sample delay.
    ///
    /// Same as [`allpass`] but uses linear interpolation for the read tap,
    /// enabling smooth modulation of the delay length.
    #[inline]
    pub fn allpass_linear(&mut self, input: f32, delay: f32, coefficient: f32) -> f32 {
        let delayed = self.read_linear(delay);
        let write_val = input + coefficient * delayed;
        self.write(write_val);
        delayed - coefficient * write_val
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p modular_core allpass_linear_unity_gain`
Expected: PASS

- [ ] **Step 5: Run all delay_line tests to check for regressions**

Run: `cargo test -p modular_core delay_line`
Expected: All 11 tests pass (10 existing + 1 new).

- [ ] **Step 6: Commit**

```bash
git -c commit.gpgsign=false add crates/modular_core/src/dsp/utils/delay_line.rs
git -c commit.gpgsign=false commit -m "feat: add allpass_linear() to DelayLine for fractional-sample modulation"
```

---

### Task 2: Add `modulation` param to `PlateParams` and wire it into the tank

**Files:**

- Modify: `crates/modular_core/src/dsp/fx/plate.rs:62-88` (add param field)
- Modify: `crates/modular_core/src/dsp/fx/plate.rs:183-328` (update `update()` method)
- Test: `crates/modular_core/src/dsp/fx/plate.rs` (add test in `mod tests`)

- [ ] **Step 1: Write the failing test for modulation effect**

Add this test at the end of the `mod tests` block in `crates/modular_core/src/dsp/fx/plate.rs`, after the `output_is_two_channels` test (before the closing `}`):

```rust
    #[test]
    fn modulation_changes_output() {
        // A plate with constant modulation offset should produce different output
        // than one without, confirming the modulation path is active.
        let n = 10000;

        let plate_no_mod = make_plate(plate_params(json!({ "input": 1.0, "decay": 3.0 })));
        let (left_no_mod, _) = collect_stereo(&**plate_no_mod, n);

        let plate_with_mod = make_plate(plate_params(
            json!({ "input": 1.0, "decay": 3.0, "modulation": 2.5 }),
        ));
        let (left_with_mod, _) = collect_stereo(&**plate_with_mod, n);

        // Outputs should differ due to modulated delay lengths
        let differs = left_no_mod
            .iter()
            .zip(left_with_mod.iter())
            .any(|(a, b)| (a - b).abs() > 1e-6);
        assert!(
            differs,
            "modulated plate should produce different output than unmodulated"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular_core modulation_changes_output`
Expected: FAIL — `modulation` field is not a recognized param.

- [ ] **Step 3: Add `modulation` field to `PlateParams`**

In `crates/modular_core/src/dsp/fx/plate.rs`, add the `modulation` field after the `diffusion` field (after line 87, before the closing `}`):

```rust
    /// external tank modulation signal (-5 to 5, default 0, not clamped)
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    modulation: Option<MonoSignal>,
```

- [ ] **Step 4: Update `Plate::update()` to use modulation**

In `crates/modular_core/src/dsp/fx/plate.rs`, in the `update()` method:

**4a.** Add modulation excursion computation after the diffusion coefficient lines (after line 210, which sets `decay_diff_2_coeff`):

```rust
        // Modulation: convert voltage to delay excursion in samples
        let mod_v = self.params.modulation.value_or(0.0);
        let mod_excursion = mod_v * (16.0 * sample_rate / REF_SAMPLE_RATE) / 5.0;
```

**4b.** Replace the left decay diffusion allpass block. Find these lines (around lines 250-255):

```rust
        // Decay diffusion allpass (left)
        let dd_l_delay = scale_delay(DECAY_DIFF_1, sample_rate, size).max(1);
        let left_after_ap =
            self.state
                .decay_diff_l
                .allpass(left_tank_in, dd_l_delay, -decay_diff_1_coeff);
```

Replace with:

```rust
        // Decay diffusion allpass (left) — fractional delay for modulation
        let dd_l_base = scale_delay(DECAY_DIFF_1, sample_rate, size) as f32;
        let dd_l_delay = (dd_l_base + mod_excursion).max(1.0);
        let left_after_ap = self.state.decay_diff_l.allpass_linear(
            left_tank_in,
            dd_l_delay,
            -decay_diff_1_coeff,
        );
```

**4c.** Replace the right decay diffusion allpass block. Find these lines (around lines 270-275):

```rust
        // Decay diffusion allpass (right)
        let dd_r_delay = scale_delay(DECAY_DIFF_2, sample_rate, size).max(1);
        let right_after_ap =
            self.state
                .decay_diff_r
                .allpass(right_tank_in, dd_r_delay, -decay_diff_2_coeff);
```

Replace with:

```rust
        // Decay diffusion allpass (right) — fractional delay for modulation
        let dd_r_base = scale_delay(DECAY_DIFF_2, sample_rate, size) as f32;
        let dd_r_delay = (dd_r_base + mod_excursion).max(1.0);
        let right_after_ap = self.state.decay_diff_r.allpass_linear(
            right_tank_in,
            dd_r_delay,
            -decay_diff_2_coeff,
        );
```

- [ ] **Step 5: Run the new test to verify it passes**

Run: `cargo test -p modular_core modulation_changes_output`
Expected: PASS

- [ ] **Step 6: Run all plate tests to check for regressions**

Run: `cargo test -p modular_core plate`
Expected: All 8 tests pass (7 existing + 1 new).

- [ ] **Step 7: Run full Rust test suite**

Run: `cargo test -p modular_core`
Expected: All tests pass (489+ unit tests, 27 integration tests).

- [ ] **Step 8: Commit**

```bash
git -c commit.gpgsign=false add crates/modular_core/src/dsp/fx/plate.rs
git -c commit.gpgsign=false commit -m "feat: add modulation param to \$plate for tank chorus/detuning"
```

---

### Task 3: Rebuild native module and regenerate TypeScript types

**Files:**

- Regenerate: `crates/modular/schemas.json`
- Regenerate: `generated/dsl.d.ts`

- [ ] **Step 1: Build native module**

Run: `yarn build-native`
Expected: Build succeeds with no errors.

- [ ] **Step 2: Regenerate TypeScript types**

Run: `yarn generate-lib`
Expected: Types regenerated, `generated/dsl.d.ts` now includes `modulation?: MonoSignal` in the `$plate` options type.

- [ ] **Step 3: Run TypeScript typecheck**

Run: `yarn typecheck`
Expected: No type errors.

- [ ] **Step 4: Run JS/TS unit tests**

Run: `yarn test:unit`
Expected: All 151+ tests pass.

- [ ] **Step 5: Commit**

```bash
git -c commit.gpgsign=false add crates/modular/schemas.json generated/dsl.d.ts
git -c commit.gpgsign=false commit -m "chore: regenerate types with \$plate modulation param"
```
