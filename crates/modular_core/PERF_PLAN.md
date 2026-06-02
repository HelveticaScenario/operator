# Pattern System Performance Plan

Phased improvement plan for the pattern_system and dependent sequencer modules
(IntervalSeq, Seq). Bench-first per project memory: every claim needs a
measurement against `bench_realworld_patterns` baseline.

## Phase 0 — Bench baseline

### P0.1 Capture baseline

- Run `cargo test -p modular_core --release --test ...` for both
  `bench_realworld_patterns` and `bench_arena_query_only` in
  `crates/modular_core/src/dsp/seq/interval_seq.rs`.
- Save ns-per-cycle for all 17 real-world patterns.
- Save median/p99 of `query_cycle_all_into` calls on master HEAD.
- Without baseline, all phase-1+ claims are speculation.

**Effort:** 1-2 hr. **Risk:** none. **Blocks:** nothing.

## Phase 1 — Cheap wins

### P1.1 Derive `Copy` on `SourceSpan`, `TimeSpan`, `Fraction`

All small POD (16-32B). Compiler may already elide clones but explicit `Copy`
gives stronger guarantees.

- `SourceSpan` (2× `usize` = 16B)
- `TimeSpan` (`begin: Fraction`, `end: Fraction` = 32B)
- `Fraction` (2× `i64` = 16B)

After: remove explicit `.clone()` calls in `with_time`, `intersection`,
arithmetic, span_arena pushes.

**Effort:** 30 min. **Risk:** low. **Benefit:** micro; compounds across span
ops and Fraction arithmetic.

### P1.2 Span arena `(usize, usize)` → `(u32, u32)`

`Seq`'s `SeqCycleStorage::span_arena: Vec<(usize, usize)>` halves to
`Vec<(u32, u32)>`. IntervalSeq's `FlatSpan` already uses `u32`.

Files: `dsp/seq/seq_value.rs::SeqCycleStorage`.

**Effort:** 30 min. **Risk:** low (source positions never exceed 4 GB).
**Benefit:** half memory, better cache locality on span walks.

### P1.3 Iterative `ArenaHapContext::walk`

Replace recursion with explicit stack walked iteratively. Stack stored in
bumpalo arena so no heap.

```
pub fn walk<F: FnMut(u8, &SourceSpan)>(&self, emit: &mut F) {
    let mut stack: SmallVec<[(u8, bool, &ArenaHapContext); 8]> = ...;
    stack.push((0, false, self));
    while let Some((pattern_idx, stripped, node)) = stack.pop() {
        match node { ... push children ... }
    }
}
```

Files: `pattern_system/hap.rs::ArenaHapContext::walk` + `walk_inner`.

**Effort:** 1 hr. **Risk:** low (semantics preserved). **Benefit:** small for
shallow trees, notable for deep `Combined` chains (5+ pattern-stack levels).

### P1.4 Bjorklund cache at param-deserializer

Same `(K, N, R)` tuple recurs across pattern instances (e.g.
`0(3,8) 1(3,8) 2(3,8)`). Hash-map cache shared per deserializer pass.

```
type BjorklundCache = HashMap<(i32, u32, i32), Arc<[bool]>>;
```

Hook into both `Pattern::new_euclid_const` and `euclid_bool`.

**Effort:** 30 min. **Risk:** low (parse-time only). **Benefit:** parse-time
(cold path); useful if Bjorklund instantiations are frequent.

## Phase 2 — Owned API removal

### P2.1 Migrate IntervalSeq parse() to `populate_cycle_storage`

`dsp/seq/interval_seq.rs:259-279` currently uses `query_cycle_all` (returns
`Vec<DspHap>`) then iterates copying fields into `CombinedHap`. Replace with
the shared `cache::populate_cycle_storage` from `dsp/seq/cache.rs` using a
convert closure matching the audio-thread path.

After: param + audio paths identical, both use bump arena.

**Effort:** 30 min. **Risk:** low (same data shape). **Benefit:** parse-time
eliminates ~1024 fresh `Vec<DspHap>` + `Arc::new` allocations. Bring code in
line with audio path.

### P2.2 Delete `Pattern::query_cycle_all` + `DspHap<T>`

After P2.1, only test/bench callers remain. Migrate those to
`query_cycle_all_into` + manual scalar conversion (or use a new bench-only
helper).

Files: `pattern_system/mod.rs::Pattern::query_cycle_all`, `pattern_system/hap.rs::DspHap`.

**Effort:** 1 hr. **Risk:** medium (touches many tests). **Benefit:** ~100
LoC removed.

### P2.3 Delete `ScaleRoot::Pattern` + `Pattern::query_at_first` + `Hap::*_f64`

`ScaleRoot::Pattern` variant has no constructor — unreachable. Delete cascade:

- `dsp/seq/scale.rs::ScaleRoot::Pattern` variant
- `pattern_system/mod.rs::Pattern::query_at_first`
- `pattern_system/hap.rs::Hap::part_contains_f64` (only used by query_at_first
  after migration) — also `whole_begin_f64`, `whole_end_f64`,
  `part_begin_f64`, `part_end_f64` if no other callers

**Effort:** 30 min. **Risk:** low. **Benefit:** ~50 LoC removed.

### P2.4 Delete `Pattern::query_arc` + `Hap<T>` + `HapContext`

Requires:
- Migrate `modular/src/lib.rs:1753` NAPI binding to consume arena haps. JS-side
  expects a structured array of haps with f64 times and span tuples — the
  NAPI layer would project from `ArenaHap`s + `extract_pattern_spans_*` into
  the JS shape without owning `Hap<T>`.
- Migrate all pattern_system tests to use a new arena-aware test helper that
  returns a `Vec<(f64, f64, T, Vec<(usize, usize)>)>` or similar shape.

**Effort:** half-day. **Risk:** high (NAPI marshaling rewrite, large test
churn). **Benefit:** ~300 LoC removed; single API surface.

Defer unless steps 1-3 leave the owned path with very few callers and the
NAPI rewrite is acceptable scope.

## Phase 3 — Specializations

### P3.1 `PureFastConst` PatternImpl variant

Mini-notation `*N` (e.g. `c*8`) is common. Currently parses as `pat.fast(
pure(Fraction::const))` which routes through `inner_join_into` — closure
dispatch per outer hap (1 outer hap per cycle, K inner haps per outer).
Constant factor means we know `K = N` ahead of time.

New variant:
```
PureFastConst(Arc<PureFastConstData<T>>)

struct PureFastConstData<T> {
    value: T,
    source_span: Option<SourceSpan>,
    factor: Fraction,           // constant N
    n: usize,                   // N
    slot_offsets: Arc<[(Fraction, Fraction)]>,  // pre-computed slot bounds
}
```

`query_into` emits N haps inline per cycle. Same idea as `EuclidConst`.

Mini-converter peephole: detect `MiniAST::Fast(Pure(value), Pure(n))` and emit
the fused variant.

**Effort:** 2 hr. **Risk:** medium (mini-converter peephole + variant body).
**Benefit:** significant on `*N`-heavy patterns. Bench `c*8 d*8 e*8 f*8`
style.

### P3.2 Voice dedup via `BitSet<MAX_CHANNELS>` keyed by `hap_index`

Current Seq/IntervalSeq `already_assigned` is O(channels × onsets):
```
let already_assigned = (0..num_channels).any(|i| voice[i].cached_hap matches ...);
```

Replace with bitset of assigned hap_indices for current cycle:
```
let mut assigned: u128 = 0;  // bits 0..MAX_CHANNELS=16
for i in 0..num_channels {
    if let Some(c) = voices[i].cached_hap && c.cached_cycle == current_cycle {
        assigned |= 1 << c.hap_index;
    }
}
// then check `assigned & (1 << hap_index) == 0` per onset
```

Note `hap_index` can exceed 128 for dense patterns — fall back to scan if so.

**Effort:** 1 hr. **Risk:** low. **Benefit:** only matters at high polyphony
with many onsets per frame; likely lost in noise. Skip unless bench flags
voice dedup as hot.

### P3.3 `span_offset/span_len: u32 → u16`

In `CombinedHap` (IntervalSeq) and `SeqCycleHap` (Seq). Per-cycle span counts
in realistic patterns ≪ 65535.

Cuts each hap from 48B → ~40B → better cache density across `slot.haps`.

`debug_assert!(span_arena.len() <= u16::MAX)` after populate to catch overflow
in dev.

**Effort:** 30 min. **Risk:** low. **Benefit:** memory + locality.

## Phase 4 — Heavy

### P4.1 Hand-rolled mini parser

Replace pest with recursive-descent parser. Pest is general-purpose and slow.

Mini grammar is complex:
- Atoms (numbers, notes, freq, voltage, MIDI, identifiers, strings)
- Operators: `*N`, `/N`, `!N`, `?P`, `@W`, `(K,N[,R])`
- Grouping: `[...]`, `<...>`, `(...)`
- Stacking: `a, b, c`
- Random choice: `a|b|c`
- Modifiers attach to elements

Test parity required against existing pest test corpus.

**Effort:** 1-2 days. **Risk:** high (subtle parse semantics easy to break).
**Benefit:** parse-time 5-10× faster. No audio-thread effect.

Skip unless patch-update latency from parse time becomes a user-visible
issue.

### P4.2 Adaptive `PARAM_CACHE_CYCLES`

`c*100` pattern uses ~16 MB param cache (1024 cycles × 150 haps × 48B + spans).

Options:
- Lower `PARAM_CACHE_CYCLES` adaptively based on observed `max_haps_per_cycle`.
- Lazy population: eagerly populate cycles 0..256 only, lazily fill 256..1024
  on first audio-thread visit (but audio-thread caches now reuse — see Seq
  iter 17 refactor — so this works).

**Effort:** 2 hr. **Risk:** medium. Audio thread may allocate if eager
horizon too low and pattern advances quickly. **Benefit:** memory.

### P4.3 Fixed-point time representation in cached haps

Replace `f64` (8B) × 4 fields in `SeqCycleHap`/`CombinedHap` with fixed-point
`i32` ticks at e.g. 1/65536-cycle resolution.

Halves hap size (48B → ~32B) and makes cycle comparisons integer.

Risks:
- Precision loss for non-integer-aligned patterns
- Voice release timing edge cases (`playhead >= whole_end` becomes integer
  compare which is exact, may help in some cases)

**Effort:** half-day. **Risk:** high. **Benefit:** memory + maybe integer
arith speed. Skip unless memory pressure real or arithmetic dominates.

## Phase 5 — Investigations (informs later phases)

### P5.1 Bumpalo chunk size

Default 4 KB. Heavy patterns may pay multiple chunk allocations per cycle
query. Try `Bump::with_capacity(64 * 1024)` at SeqState/IntervalSeqState
construction.

**Effort:** 15 min + bench. **Risk:** low.

### P5.2 PatternImpl::Arena dispatch frequency

Audit how often `PatternImpl::Arena` (closure dispatch via `Arc<dyn Fn>`)
arm is hit vs specialized variants. Inject a `#[cfg(debug_assertions)]`
counter, run benches, log frequencies. If Arena dominates, more specialized
variants needed (informs P3.1 priority).

**Effort:** 30 min. **Risk:** none.

### P5.3 `Pattern.clone` cost

Each `Pattern::new_*` Arc-wraps its data. Deep combinator chains do many
Arc-clones during construction.

Consider wrapping the entire `Pattern<T>` body in `Arc<...>` so `Clone` is
O(1) atomic refcount bump regardless of internal variant.

**Effort:** 1 hr investigation + bench. **Risk:** medium (forces all
consumers to deref through Arc).

## Recommended order

1. **P0.1** baseline (mandatory first)
2. **P1.1, P1.2, P1.3, P1.4** (bench between each — confirm wins or revert)
3. **P2.1** (cheap and clean)
4. **P2.2, P2.3** (cleanup, lower risk after P2.1)
5. **P3.1** (likely biggest hot-path win)
6. **P3.3** (easy memory win)
7. **P5.1, P5.2, P5.3** (investigations — defer commits, just measure)
8. **P3.2** (only if P5 shows voice dedup hot)
9. **P2.4** (only if NAPI rewrite worth the LoC win)
10. **P4.x** (defer unless specific pressure)

## Total effort

| Phase | Items | Effort |
|-------|-------|--------|
| 0 | Bench baseline | 1-2 hr |
| 1 | P1.1-P1.4 | ~3 hr |
| 2 (P2.1-P2.3) | Owned API cleanup | ~2 hr |
| 3 (P3.1, P3.3) | Specializations | ~3 hr |
| 5 | Investigations | ~2 hr |
| 2.4 | NAPI migration | half-day |
| 4 | Heavy items | 1-3 days each, defer |

Phases 0-3 + P5 ≈ one focused day. Expected hot-path improvement (speculative,
needs P0 to confirm): 1.5-3× on `*N`-heavy patterns. Cheap wins compound;
specializations target specific hot mini-notation cases.

## Rules

- No perf claim without a measurement against the captured baseline.
- After each landed item: re-run benches, compare to baseline, document
  delta in commit message.
- If an item regresses or shows < 3% improvement: revert. Compounding micro-
  wins are fine; net regressions on hot paths are not.
- Memory wins are also valid wins — flag them as such in commits.
