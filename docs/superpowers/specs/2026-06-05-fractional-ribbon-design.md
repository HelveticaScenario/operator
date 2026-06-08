# Fractional ribbon window for `$cycle`

## Goal

Let the `$cycle` sequencer's `ribbon: [offset, length]` loop window accept
fractional cycle values, not just whole-cycle integers. Currently
`ribbon` is `(u64, u64)`; `ribbon:[0.5, 1.5]` is rejected. After this change a
fractional ribbon defines a continuous-time loop window (the seam may fall
mid-cycle) and integer ribbons behave exactly as they do today.

## Motivation

A whole-cycle-only loop window cannot express odd-length or phase-shifted
loops (e.g. a 1.5-cycle loop, or a window starting half a cycle in). Both
offset and length should be fractional.

## Design

### Param type

- `ribbon: (f64, f64)` (was `(u64, u64)`), `(offset, length)` in cycles.
- Default `(0.0, 1024.0)` — unchanged cost (1024 baked cycles).
- Bounds constants become `f64`: `MAX_RIBBON_LENGTH = 8192.0`,
  `MAX_RIBBON_OFFSET = 1_000_000.0`, `DEFAULT_RIBBON_LENGTH = 1024.0`.

### Audio-thread fold (`Seq::update`)

The window folds the monotonic clock with a continuous-time modulo instead of
an integer one:

```text
raw_clamped = raw.max(0.0)
phase       = raw_clamped.rem_euclid(length)   // [0, length)
pos         = offset + phase                    // [offset, offset+length)
pos_cycle   = pos.floor() as i64                // integer cycle for storage lookup
logical     = pos                               // playhead in the pattern's absolute frame
current_cycle = pos_cycle                        // dedup key
```

- Onset detection compares `logical` against `hap.part_begin/part_end` (the
  baked cycle's absolute frame) — unchanged.
- `already_assigned` dedup keys off `current_cycle` — unchanged.
- Release stays in the monotonic `raw` frame:
  `raw_begin = raw - (logical - whole_begin)`, `raw_end = raw + (whole_end - logical)`.
  A note crossing the (now possibly mid-cycle) seam plays its full length into
  the loop restart, exactly as in the integer design.

Clock cycle 0 still plays the window start (`phase = 0 → pos = offset`):
`offset` selects which pattern cycles are baked and where the window sits; the
loop always begins at clock 0.

### Backward compatibility (proof obligation)

For integer `offset` and `length`:
`pos.floor() == offset + (floor(raw) % length)` and `frac(pos) == frac(raw)`,
which equal today's `current_cycle` and `logical`. The seven existing ribbon
tests must pass **unmodified** (except the validation test below, where one
input changes meaning).

### Baking

The window touches integer cycles `[floor(offset), ceil(offset + length))`.
Bake one `SeqCycleStorage` per integer cycle in that range.

- `base = floor(offset) as i64`; `cached_haps[i]` holds cycle `base + i`.
- `SeqPatternParam::bake(offset, length)` computes `base` and `end =
  ceil(offset + length) as i64`, baking cycles `base..end`.
- `get_cycle_storage(cycle, base, cached)` indexes `cached[cycle - base]`
  (signature changes from `offset: u64` to `base: i64`).
- Bake count = `end - base` ≤ ~8194 with `length ≤ 8192` — bounded, all on the
  main thread (`deserr` validate hook), never on the audio thread.

### Validation (`seq_bake_ribbon`)

`f64` no longer rejects negative or fractional values structurally, so the hook
owns all bounds. Reject with a `ribbon`-keyed `ModuleParamErrors`:

- `offset` or `length` not finite (NaN / ±∞) → "ribbon values must be finite"
- `length <= 0.0` → "ribbon loop length must be greater than 0" (existing)
- `length > MAX_RIBBON_LENGTH` → "...8192 cycles or fewer" (existing)
- `offset < 0.0` → "ribbon offset must be 0 or greater" (new)
- `offset > MAX_RIBBON_OFFSET` → "...1000000 cycles or fewer" (existing)

### Schema / DSL surface

- `yarn build-native` regenerates `schemas.json`: `prefixItems` become
  `type: number`, default `[0.0, 1024.0]`.
- `yarn generate-lib`: the generated DSL type stays `ribbon?: [number, number]`
  (TS `number` already covered both). No hand-written TS references `ribbon`,
  so there is no DSL-surface change — fractional values simply stop erroring.

## Testing

- **Unchanged:** `seq_ribbon_default_window_plays_pattern_through`,
  `seq_ribbon_loops_window`, `seq_ribbon_offset_window_plays_and_loops`,
  `seq_ribbon_wrap_note_plays_full_length_then_releases_once`,
  `seq_ribbon_note_longer_than_window_plays_full_then_gaps`,
  `seq_ribbon_note_dividing_window_loops_seamlessly`.
- **Updated** `seq_ribbon_rejects_invalid_bounds`: `[0.5, 4]` flips from
  rejected to **valid**; `[-1, 4]` still rejected (now by the hook, not
  structurally). Add NaN / ∞ rejection cases.
- **New:**
  - Fractional length: `ribbon:[0, 1.5]` loops with period 1.5 cycles (seam at
    mid-cycle); the value at clock pos 0.25, 1.25, and 2.75 follows the
    folded window.
  - Fractional offset: `ribbon:[0.5, 2]` — window starts half a cycle into the
    pattern and loops.

## Out of scope

- Changing the "play full length across the seam" release semantics.
- Any change to `$step`, `$track`, or other sequencer modules (they do not use
  the ribbon).
