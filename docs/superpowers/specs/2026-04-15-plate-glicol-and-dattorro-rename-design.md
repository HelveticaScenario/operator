# Design: `$plate` (Glicol-style reverb) and `$dattorro` rename

## Summary

Rename the existing `$plate` module to `$dattorro` (it's a faithful Dattorro paper implementation), then create a new `$plate` module that implements the Glicol plate reverb topology. The Glicol version has a denser, more complex feedback network that produces a thicker, warmer reverb tail compared to the minimal Dattorro.

## Part 1: Rename existing `$plate` → `$dattorro`

- Rename file `plate.rs` → `dattorro.rs`
- Rename struct `Plate` → `Dattorro`, state `PlateState` → `DattorroState`, params `PlateParams` → `DattorroParams`, outputs `PlateOutputs` → `DattorroOutputs`
- Change module name from `$plate` to `$dattorro` in the `#[module]` attribute
- Update `fx/mod.rs`: `pub mod plate;` → `pub mod dattorro;`; update all three registration functions
- Update `dsp_fresh_tests.rs` minimal params: `$plate` → `$dattorro`
- All params, tests, behavior remain identical

## Part 2: New shared primitive — `OnePole`

Add `crates/modular_core/src/dsp/utils/one_pole.rs` with a simple one-pole lowpass filter:

```rust
pub struct OnePole {
    coeff: f32,   // filter coefficient (0..1), higher = more HF passed
    state: f32,   // filter state
}
```

Methods: `new(coeff)`, `process(input) -> f32`, `set_coeff(coeff)`, `Default` (coeff=0.5, state=0.0).

The filter equation: `y[n] = x[n] * coeff + y[n-1] * (1 - coeff)`.

Export from `utils/mod.rs` as `pub mod one_pole;`.

## Part 3: New `$plate` module — Glicol-inspired topology

### Signal flow

The topology comes from [Glicol's plate.rs](https://github.com/chaosprint/glicol/blob/main/rs/synth/src/node/effect/plate.rs) and the [dattorro-vst-rs](https://github.com/chaosprint/dattorro-vst-rs/blob/main/src/lib.rs) VST plugin. Both use the same network. We implement it as monolithic sample-by-sample DSP (not a sub-graph).

```
Input → stereo sum (even ch=L, odd ch=R) → mono
  → Input OnePole LPF (bandwidth param, default coeff 0.7)
  → 50ms fixed predelay
  → 4 input diffusion allpasses:
      AP(4.771ms, gain 0.75)
      AP(3.595ms, gain 0.75)
      AP(12.72ms, gain 0.625)
      AP(9.307ms, gain 0.625)
  → Add feedback (from end of Line F)
  → Modulated allpass "wet8" (100ms base, gain 0.7, +mod_excursion)

Line A: DelayN(394) [tap: aa] → DelayN(2800) [tap: ab] → DelayN(1204) [tap: ac]
Line B: DelayN(2000) → OnePole(damping, default 0.1) → AP(7.596ms, 0.5) [tap: ba3]
         → AP(35.78ms, 0.5) [tap: bb] → Modulated AP(100ms base, 0.5, +mod_excursion)
Line C: DelayN(179) [tap: ca] → DelayN(2679) [tap: cb] → DelayN(3500) → Mul(decay, default 0.3)
Line D: AP(30ms, 0.7) → DelayN(522) [tap: da2] → DelayN(2400) [tap: db] → DelayN(2400) [tap: dc]
Line E: OnePole(damping, default 0.1) → AP(6.2ms, 0.7) [tap: ea2] → AP(34.92ms, 0.7) [tap: eb]
Line F: AP(20.4ms, 0.7) → DelayN(1578) [tap: fa2] → DelayN(2378) [tap: fb]
         → DelayN(2500) → Mul(decay, default 0.3) → feedback to input

Output tap matrix (matches Glicol exactly):
  Left  = +aa +ab +cb -(bb + db + ea2 + fa2)
  Right = +da2 +db +fb -(eb + ab + ba3 + ca)
```

All delay lengths from the Glicol source are in samples at 48kHz (for `DelayN`) or milliseconds (for `DelayMs`/`AllPassFilterGain`). These are sample-rate-scaled at init time.

### Parameters

All non-input params are optional with `Option<MonoSignal>` + `#[deserr(default)]`.

| Param        | Type                 | Default voltage | Range | Algorithm mapping                                                                   | Glicol default coeff |
| ------------ | -------------------- | --------------- | ----- | ----------------------------------------------------------------------------------- | -------------------- |
| `input`      | PolySignal           | required        | —     | Audio in                                                                            | —                    |
| `bandwidth`  | Option\<MonoSignal\> | 1.67            | -5..5 | Input OnePole coeff: maps -5..5 → 0.1..0.9999                                       | 0.7                  |
| `damping`    | Option\<MonoSignal\> | 4.08            | -5..5 | Tank OnePole coeffs: maps -5..5 → 0.9999..0.01 (inverted — higher V = more damping) | 0.1                  |
| `decay`      | Option\<MonoSignal\> | -1.997          | -5..5 | Feedback Mul gains: maps -5..5 → 0.0..0.9999                                        | 0.3                  |
| `modulation` | Option\<MonoSignal\> | 0               | -5..5 | External mod excursion: ±5V → ±5.5 samples (at 48kHz)                               | 0.1Hz sine ±5.5      |

**Default mapping**: All params use simple linear `map_range`. The default voltages are set to the values that produce the Glicol default coefficients. For example, bandwidth default 1.67V maps to coeff 0.7 via `map_range(1.67, -5, 5, 0.1, 0.9999) ≈ 0.7`.

**Modulation scaling**: At 48kHz, `±5V → ±5.5 samples`. This means `$sine('0.1hz')` (which outputs ±5V) wired to `modulation` reproduces the exact Glicol internal LFO behavior. The excursion scales with sample rate: `5.5 * (sr / 48000)`.

**100% wet output** — no mix param. The Glicol VST has mix, but in our system users mix externally via `.send()`, `.pipeMix()`, `$mix`, etc.

### State

```rust
struct PlateState {
    // Input section
    input_lpf: OnePole,          // bandwidth filter
    predelay: DelayLine,         // 50ms fixed predelay
    input_diff: [DelayLine; 4],  // 4 input diffusion allpasses

    // Feedback junction
    feedback: f32,

    // Modulated allpasses (wet8 and bc from Glicol)
    mod_ap_1: DelayLine,         // wet8: 100ms, gain 0.7
    mod_ap_2: DelayLine,         // bc: 100ms, gain 0.5

    // Line A: 3 delay lines
    line_a: [DelayLine; 3],      // 394, 2800, 1204 samples

    // Line B: delay + OnePole + 2 allpasses
    line_b_delay: DelayLine,     // 2000 samples
    line_b_lpf: OnePole,         // damping
    line_b_ap: [DelayLine; 2],   // 7.596ms, 35.78ms

    // Line C: 3 delay lines
    line_c: [DelayLine; 3],      // 179, 2679, 3500 samples

    // Line D: 1 allpass + 3 delay lines
    line_d_ap: DelayLine,        // 30ms
    line_d_delay: [DelayLine; 3], // 522, 2400, 2400 samples

    // Line E: OnePole + 2 allpasses
    line_e_lpf: OnePole,         // damping
    line_e_ap: [DelayLine; 2],   // 6.2ms, 34.92ms

    // Line F: 1 allpass + 3 delay lines
    line_f_ap: DelayLine,        // 20.4ms
    line_f_delay: [DelayLine; 3], // 1578, 2378, 2500 samples

    // Parameter smoothing
    smoothed_bandwidth: Clickless,
    smoothed_damping: Clickless,
    smoothed_decay: Clickless,

    // DC blocking HPF (20Hz) on output
    dc_prev_in_l: f32,
    dc_prev_in_r: f32,
    dc_prev_out_l: f32,
    dc_prev_out_r: f32,
    dc_block_coeff: f32,

    sample_rate: f32,
}
```

### Implementation conventions

- `has_init` flag for main-thread delay line allocation
- `channels = 2` for hardcoded stereo output
- `Clickless` smoothing on bandwidth, damping, decay
- DC blocking HPF at 20Hz on both output channels
- Modulated allpasses use `allpass_linear()` for fractional-sample reads
- Fixed delay lines use integer `read()` (no need for interpolation on fixed delays)
- All ms-based delays convert to samples at the actual sample rate in `init()`
- All sample-based delays (from `DelayN` in Glicol, which assumes 48kHz) scale by `sr / 48000`
- No `size` or `predelay` param — these are part of the fixed topology

### DSL usage

```js
// Basic usage — all Glicol defaults
$plate($saw('c3')).out();

// With parameters
$plate($saw('c3'), { decay: 2, damping: -1, bandwidth: 1 }).out();

// With external LFO modulation (reproduces Glicol's internal LFO)
$plate($saw('c3'), { modulation: $sine('0.1hz') }).out();

// External mix (since $plate is 100% wet)
$saw('c3').send('verb', 0.3);
$plate($input('verb')).out();
```

## Files to create/modify

### New files:

- `crates/modular_core/src/dsp/utils/one_pole.rs` — OnePole filter primitive
- `crates/modular_core/src/dsp/fx/plate.rs` — New Glicol-style plate reverb

### Renamed:

- `crates/modular_core/src/dsp/fx/plate.rs` → `crates/modular_core/src/dsp/fx/dattorro.rs`

### Modified:

- `crates/modular_core/src/dsp/utils/mod.rs` — add `pub mod one_pole;`
- `crates/modular_core/src/dsp/fx/mod.rs` — rename `plate` → `dattorro`, add new `plate`
- `crates/modular_core/tests/dsp_fresh_tests.rs` — rename `$plate` → `$dattorro`, add new `$plate`
- `crates/modular/schemas.json` — regenerated
- `generated/dsl.d.ts` — regenerated
