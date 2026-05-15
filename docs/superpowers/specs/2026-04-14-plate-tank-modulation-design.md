# Design: External Tank Modulation for `$plate`

## Summary

Add an optional `modulation` param to the `$plate` module that allows an external signal (typically a slow LFO) to modulate the decay diffusion allpass delay lengths. This creates the chorusing/detuning that prevents metallic ringing in the reverb tank and adds lushness — a key element of the original Dattorro algorithm that was deferred from the initial implementation.

## Param Spec

- **Name**: `modulation`
- **Type**: `Option<MonoSignal>` with `#[deserr(default)]`
- **Default**: `0.0` (no modulation)
- **Range annotation**: `(-5.0, 5.0)` — for DSL metadata only, not clamped
- **Behavior**: Bipolar signal mapped internally to a delay excursion in samples. Not clamped — values beyond +/-5V produce larger excursions for creative effects.

## Signal Mapping

The modulation signal is mapped from voltage to a delay offset in samples:

```
excursion_samples = signal_value * (16.0 * sample_rate / 29761.0) / 5.0
```

At 48kHz:

- 0V → 0 samples offset (no modulation)
- 5V → ~25.8 samples offset
- -5V → ~-25.8 samples offset
- 10V → ~51.6 samples offset (unclamped, extreme chorus)

The reference excursion of 16 samples at 29761 Hz comes from Dattorro's paper. Scaling by `sample_rate / REF_SAMPLE_RATE` keeps the perceptual effect consistent across sample rates.

## Implementation Changes

### 1. `DelayLine::allpass_linear()`

New method on `DelayLine` in `crates/modular_core/src/dsp/utils/delay_line.rs`:

```rust
pub fn allpass_linear(&mut self, input: f32, delay: f32, coefficient: f32) -> f32 {
    let delayed = self.read_linear(delay);
    let write_val = input + coefficient * delayed;
    self.write(write_val);
    delayed - coefficient * write_val
}
```

Same structure as `allpass()` but uses `read_linear()` for fractional-sample delay reading. The write position is unaffected — only the read tap modulates.

### 2. `PlateParams` — add `modulation` field

```rust
/// external tank modulation signal (-5 to 5, default 0, not clamped)
#[signal(default = 0.0, range = (-5.0, 5.0))]
#[deserr(default)]
modulation: Option<MonoSignal>,
```

### 3. `Plate::update()` — apply modulation to decay diffusion stages

Compute the excursion from the modulation param, then use `allpass_linear()` instead of `allpass()` for the two decay diffusion allpass stages:

```rust
let mod_v = self.params.modulation.value_or(0.0);
let mod_excursion = mod_v * (16.0 * sample_rate / REF_SAMPLE_RATE) / 5.0;

// Left decay diffusion — use fractional delay
let dd_l_base = scale_delay(DECAY_DIFF_1, sample_rate, size) as f32;
let dd_l_delay = (dd_l_base + mod_excursion).max(1.0);
let left_after_ap = self.state.decay_diff_l.allpass_linear(
    left_tank_in, dd_l_delay, -decay_diff_1_coeff);

// Right decay diffusion — use fractional delay
let dd_r_base = scale_delay(DECAY_DIFF_2, sample_rate, size) as f32;
let dd_r_delay = (dd_r_base + mod_excursion).max(1.0);
let right_after_ap = self.state.decay_diff_r.allpass_linear(
    right_tank_in, dd_r_delay, -decay_diff_2_coeff);
```

The `.max(1.0)` prevents reading at delay 0 or negative, which would be invalid. Everything else in the tank (input diffusers, main delay lines, output taps) stays integer-delay.

### 4. Regenerate types

Run `yarn build-native && yarn generate-lib` to update `schemas.json` and `generated/dsl.d.ts`.

### 5. Tests

- **`DelayLine::allpass_linear` unit test**: verify energy preservation (same approach as existing `allpass_unity_gain` test).
- **`$plate` modulation test**: verify that a plate with constant modulation offset produces different output than one without, confirming the modulation path is active.

## Usage Example

```js
// Classic lush plate: 0.5Hz LFO modulating the tank
$plate($saw('c3'), { decay: 3, modulation: $sine('0.5hz') }).out();

// Extreme chorus effect: faster LFO, higher amplitude
$plate($saw('c3'), { decay: 3, modulation: $sine('2hz').mul(2) }).out();

// No modulation (backwards compatible, same as before)
$plate($saw('c3'), { decay: 3 }).out();
```

## Scope

This change is small and self-contained:

- 1 new method on `DelayLine` (~6 lines)
- 1 new param on `PlateParams` (~3 lines)
- ~10 lines changed in `Plate::update()`
- Type regeneration
- 2 new tests
