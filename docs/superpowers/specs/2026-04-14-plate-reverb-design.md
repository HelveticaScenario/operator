# `$plate` — Dattorro Plate Reverb Module

## Summary

A stereo plate reverb module implementing Jon Dattorro's plate reverberator algorithm ("Effect Design Part 1: Reverberator and Other Filters", JAES 1997). Lives in the `fx` category alongside `$fold`, `$cheby`, and `$segment`. Always 100% wet output — users mix externally.

## Algorithm

The Dattorro plate reverb consists of:

1. **Stereo input summing**: Even polyphonic channels sum to left, odd to right
2. **Predelay**: One delay line per stereo channel (0–500ms)
3. **Input diffusers**: 4 cascaded allpass filters that smear the summed mono input in time
4. **Tank**: Two cross-coupled delay paths (left/right), each containing:
    - A decay-diffusion allpass filter
    - A long delay line with multiple output taps
    - A one-pole lowpass damping filter
    - Decay feedback coefficient
5. **Output taps**: Multiple taps from both tank delay lines, summed to produce decorrelated stereo L/R

Cross-feedback (left tank output feeds right tank input and vice versa) creates the stereo image.

## Shared Delay Primitives

A new `DelayLine` struct in `crates/modular_core/src/dsp/utils/delay_line.rs`. The existing `utils.rs` will be converted to a `utils/` directory with `mod.rs`.

```rust
pub struct DelayLine {
    buffer: Vec<f32>,
    write_ptr: usize,
    mask: usize,  // buffer.len() - 1 (power-of-2)
}
```

Methods: `new(max_delay)`, `write(sample)`, `read(delay)`, `read_linear(delay)`, `allpass(input, delay, coefficient)`, `clear()`. Buffer sized to next power of 2 for bitwise-AND wrapping.

## Module Definition

- **Name**: `$plate`
- **Category**: `fx`
- **Channels**: Hardcoded to 2 (stereo output)
- **Flags**: `has_init` (delay line allocation on main thread)
- **Positional args**: `input`

## Parameters

| Param       | Type         | Default | Range       | Description                                    |
| ----------- | ------------ | ------- | ----------- | ---------------------------------------------- |
| `input`     | `PolySignal` | —       | —           | Audio input. Even channels → left, odd → right |
| `decay`     | `MonoSignal` | 0.0     | -5.0 to 5.0 | Reverb decay time                              |
| `damping`   | `MonoSignal` | 0.0     | -5.0 to 5.0 | High-frequency damping                         |
| `size`      | `MonoSignal` | 0.0     | -5.0 to 5.0 | Room size (delay length scaling)               |
| `predelay`  | `MonoSignal` | 0.0     | 0.0 to 0.5  | Predelay time in seconds                       |
| `diffusion` | `MonoSignal` | 3.5     | 0.0 to 5.0  | Input diffusion amount                         |

Output is always 100% wet. No mix parameter.

Bipolar params (decay, damping, size) use the full -5 to 5V range. Internal voltage-to-coefficient mappings use `map_range` to convert the full bipolar range to algorithm-appropriate coefficient ranges:

- `decay` → -5..5V → decay coefficient ~0.0 to ~0.9999
- `damping` → -5..5V → lowpass bandwidth coefficient ~1.0 to ~0.0
- `size` → -5..5V → delay length multiplier ~0.25x to ~2.0x
- `predelay` → 0.0 to 0.5 seconds
- `diffusion` → 0..5V → allpass coefficients ~0.0 to ~0.75

## Output

Single default `PolyOutput` with 2 channels:

- Channel 0: Left reverb output
- Channel 1: Right reverb output

## State

Pre-allocated in `init(sample_rate)`:

- 2 predelay lines (L/R)
- 4 input diffuser allpass delay lines
- 2 decay diffusion allpass delay lines (L/R tank)
- 2 long delay lines (L/R tank)
- 2 damping filter states (f32)
- 2 feedback values (f32)

All delay lengths from Dattorro's paper, scaled from 29761 Hz reference to actual sample rate, then multiplied by the size parameter.

## Dattorro Delay Lengths (at 29761 Hz reference)

**Input diffusers**: 142, 107, 379, 277 samples
**Left tank**: decay_ap=672, delay=4453
**Right tank**: decay_ap=908, delay=4217
**Output taps**: Specific tap indices from Dattorro's Table 1

## Signal Flow

```
Input (PolySignal, N channels)
  ├── ch0, ch2, ch4... → sum → left_in
  └── ch1, ch3, ch5... → sum → right_in

left_in → predelay_l ─┐
right_in → predelay_r ─┤
                        └→ (left_predelayed + right_predelayed) / 2
                           → Input Diffusers (4x allpass)
                           → diffused

Left Tank:                    Right Tank:
  diffused + feedback_r         diffused + feedback_l
  → decay_ap_l                  → decay_ap_r
  → delay_l                     → delay_r
  → damp_lp_l                   → damp_lp_r
  → × decay_coeff               → × decay_coeff
  → feedback_l                  → feedback_r

Output (stereo, from multiple tank taps):
  L = tap(delay_l, t1) - tap(decay_ap_r, t2) + tap(delay_r, t3)
      - tap(delay_l, t4) - tap(delay_r, t5) - tap(decay_ap_l, t6) - tap(delay_l, t7)
  R = tap(delay_r, t8) - tap(decay_ap_l, t9) + tap(delay_l, t10)
      - tap(delay_r, t11) - tap(delay_l, t12) - tap(decay_ap_r, t13) - tap(delay_r, t14)
```

(Exact tap indices from Dattorro Table 1, scaled to sample rate.)

## File Changes

1. **Convert `utils.rs` to `utils/` directory**:
    - Move `crates/modular_core/src/dsp/utils.rs` → `crates/modular_core/src/dsp/utils/mod.rs`
    - Add `crates/modular_core/src/dsp/utils/delay_line.rs`

2. **New module file**:
    - `crates/modular_core/src/dsp/fx/plate.rs`

3. **Register module**:
    - Update `crates/modular_core/src/dsp/fx/mod.rs` (add `mod plate;` + register in `install_constructors`, `install_params_deserializers`, `schemas`)

4. **No DSL factory changes needed** — auto-generated from schema.

5. **No N-API changes needed** — module registration is automatic through `install_constructors`.

## Testing

**DelayLine primitive tests** (`utils/delay_line.rs`):

- Write/read at various delays
- Allpass correctness
- Power-of-2 wrapping
- Clear
- Linear interpolation

**Plate module tests** (`fx/plate.rs`):

- Impulse response: non-zero decaying output
- Decay parameter: higher = longer tail
- Silence in/out
- Stereo separation: L != R
- Even/odd channel summing
- DC stability: no offset accumulation

## Safety

- All delay lines allocated in `init()` (main thread)
- `update()` (audio thread) only reads/writes pre-allocated buffers
- No heap allocation in audio path
- State struct fields with `DelayLine`s use `Default` trait (empty buffers) for serialization safety
