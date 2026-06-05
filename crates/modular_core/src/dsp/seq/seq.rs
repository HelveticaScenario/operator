//! Seq module - A Strudel/TidalCycles style sequencer using the new pattern system.
//!
//! This module sequences pitch values using mini notation patterns with support for:
//! - V/Oct voltage values (pre-converted from MIDI/notes at parse time)
//!
//! The sequencer queries the pattern at the current playhead position and outputs:
//! - CV: V/Oct pitch (A0 = 0V)
//! - Gate: High while note is active
//! - Trig: Short pulse at note onset

use std::cmp::Ordering;

use deserr::Deserr;
use schemars::JsonSchema;

use arrayvec::ArrayVec;

use crate::{
    MonoSignal,
    dsp::utils::{TempGate, TempGateState, min_gate_samples},
    param_errors::ModuleParamErrors,
    poly::{MonoSignalExt, PORT_MAX_CHANNELS, PolyOutput},
};

use super::seq_value::{SeqCycleStorage, SeqPatternParam, SeqValue};

/// Cached hap copied into voice state as scalars. Voices can hold these
/// by value without keeping the cycle storage alive.
#[derive(Clone, Copy, Debug)]
struct CachedHap {
    /// Index of this hap within the owning cycle's `haps` vector.
    hap_index: u32,
    /// The baked cycle this hap belongs to, in `[offset, offset+length)`.
    cached_cycle: i64,
    /// `whole_begin` in the pattern's logical (cycle) frame. Kept for
    /// `get_state` highlight geometry; NOT used for release, since the
    /// logical frame wraps at the ribbon seam.
    whole_begin: f64,
    /// `whole_end` in the logical frame (see `whole_begin`).
    whole_end: f64,
    /// Note onset in the monotonic clock (`raw`) frame.
    raw_begin: f64,
    /// Note end in the monotonic clock (`raw`) frame. Release keys off `raw`
    /// so a note plays its full length across the ribbon wrap, where the
    /// logical frame is non-monotonic.
    raw_end: f64,
    /// Cached value for CV/rest detection.
    value: SeqValue,
}

impl CachedHap {
    fn contains(&self, raw: f64) -> bool {
        raw >= self.raw_begin && raw < self.raw_end
    }

    fn get_cv(&self) -> Option<f64> {
        match self.value {
            SeqValue::Voltage(v) => Some(v),
            SeqValue::Rest => None,
        }
    }

    fn is_rest(&self) -> bool {
        matches!(self.value, SeqValue::Rest)
    }
}

/// Per-voice state for polyphonic sequencer.
#[derive(Clone)]
struct VoiceState {
    /// Cached hap for this voice's current playhead position.
    cached_hap: Option<CachedHap>,
    /// Gate generator for this voice.
    gate: TempGate,
    /// Trigger generator for this voice.
    trigger: TempGate,
    /// Whether this voice is currently active (playing a note).
    active: bool,
    /// Timestamp when this voice was last assigned (for LRU stealing).
    last_assigned: f64,
}

impl Default for VoiceState {
    fn default() -> Self {
        Self {
            cached_hap: None,
            gate: TempGate::new_gate(TempGateState::Low),
            trigger: TempGate::new_gate(TempGateState::Low),
            active: false,
            last_assigned: 0.0,
        }
    }
}

fn default_channels() -> usize {
    4
}

/// Default ribbon loop length in cycles. Same memory/cost as the old
/// param cache (cycles `0..1024` baked at parse time).
const DEFAULT_RIBBON_LENGTH: u64 = 1024;

/// Largest ribbon loop length. The whole window bakes synchronously on the
/// main thread on every pattern/ribbon edit, so it is capped.
const MAX_RIBBON_LENGTH: u64 = 8192;

/// Largest ribbon offset. Generous, and keeps `offset + length` exact in
/// `f64` (well under 2^53).
const MAX_RIBBON_OFFSET: u64 = 1_000_000;

fn default_ribbon() -> (u64, u64) {
    (0, DEFAULT_RIBBON_LENGTH)
}

#[derive(Clone, Deserr, ChannelCount, JsonSchema, Connect, Debug, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = seq_bake_ribbon -> ModuleParamErrors)]
pub struct SeqParams {
    /// pattern string in mini-notation
    pattern: SeqPatternParam,
    /// playhead position (driven by the global clock)
    #[default_connection(module = RootClock, port = "playhead", channels = [0, 1])]
    #[signal(range = (0.0, 1.0))]
    #[deserr(default)]
    playhead: Option<MonoSignal>,
    /// Number of polyphonic voices (1-16)
    #[deserr(default)]
    pub channels: Option<usize>,
    /// loop window [offset, length] in cycles
    #[serde(default = "default_ribbon")]
    #[deserr(default = default_ribbon())]
    ribbon: (u64, u64),
    /// The pattern string (used for serialization)
    #[serde(skip)]
    #[deserr(skip)]
    #[schemars(skip)]
    pub pattern_source: String,
}

/// Struct-level `deserr` validate hook. Runs after every field (and its
/// default) is deserialized but before channel-count derivation, so both
/// `pattern` and `ribbon` are present. Validates the ribbon bounds, then
/// bakes the pattern's haps for the loop window `[offset, offset+length)`.
fn seq_bake_ribbon(
    mut params: SeqParams,
    _location: deserr::ValuePointerRef,
) -> Result<SeqParams, ModuleParamErrors> {
    let (offset, length) = params.ribbon;
    if length == 0 {
        let mut err = ModuleParamErrors::default();
        err.add(
            "ribbon".to_string(),
            "ribbon loop length must be greater than 0".to_string(),
        );
        return Err(err);
    }
    if length > MAX_RIBBON_LENGTH {
        let mut err = ModuleParamErrors::default();
        err.add(
            "ribbon".to_string(),
            format!("ribbon loop length must be {MAX_RIBBON_LENGTH} cycles or fewer"),
        );
        return Err(err);
    }
    if offset > MAX_RIBBON_OFFSET {
        let mut err = ModuleParamErrors::default();
        err.add(
            "ribbon".to_string(),
            format!("ribbon offset must be {MAX_RIBBON_OFFSET} cycles or fewer"),
        );
        return Err(err);
    }
    params.pattern.bake(offset, length);
    Ok(params)
}

/// Channel count derivation for Seq.
///
/// Analyzes the pattern to determine maximum polyphony by running 90 cycles
/// of the pattern and counting maximum simultaneous haps.
///
/// This is called by TypeScript to derive channel count from params.
/// Inside Seq::update(), we read params.channels directly (which TypeScript
/// will have already set based on this analysis, or user explicitly set).
pub fn seq_derive_channel_count(params: &SeqParams) -> usize {
    // If channels was explicitly set (non-default), use that
    if let Some(channels) = params.channels {
        return channels.clamp(1, PORT_MAX_CHANNELS);
    }

    // Otherwise, analyze pattern polyphony using cached haps
    let cached = params.pattern.cached_haps();
    if cached.is_empty() {
        return default_channels();
    }

    const MAX_POLYPHONY: usize = 16;

    // Sweep line algorithm using f64 coordinates from cached haps
    let mut events: Vec<(f64, i32)> = Vec::new();

    for cycle_storage in cached {
        for hap in cycle_storage.haps.iter() {
            if hap.value.is_rest() {
                continue;
            }
            events.push((hap.part_begin, 1)); // +1 at start
            events.push((hap.part_end, -1)); // -1 at end
        }
    }

    // Sort by time, with ends (-1) before starts (+1) at same time
    events.sort_by(|a, b| {
        match a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal) {
            Ordering::Equal => a.1.cmp(&b.1), // -1 comes before +1
            other => other,
        }
    });

    // Sweep through events tracking current and max polyphony
    let mut current: usize = 0;
    let mut max_simultaneous: usize = 0;

    for (_time, delta) in events {
        if delta > 0 {
            current += 1;
            max_simultaneous = max_simultaneous.max(current);
            if max_simultaneous >= MAX_POLYPHONY {
                return MAX_POLYPHONY;
            }
        } else {
            current = current.saturating_sub(1);
        }
    }
    max_simultaneous.max(1)
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SeqOutputs {
    #[output("cv", "pitch output in V/Oct", default)]
    cv: PolyOutput,
    #[output("gate", "high (5 V) while a note is active, low (0 V) otherwise", range = (0.0, 5.0))]
    gate: PolyOutput,
    #[output("trig", "short pulse (5 V) at the start of each note", range = (0.0, 5.0))]
    trig: PolyOutput,
}

/// Pattern sequencer using mini-notation strings.
///
/// Write rhythmic and melodic patterns using a compact text syntax ported
/// from TidalCycles/Strudel. The pattern loops each **cycle** and supports
/// polyphony — overlapping notes are automatically allocated to separate
/// output channels.
///
/// ## Cycles
///
/// A **cycle** is one full traversal of the pattern. The playhead position
/// determines timing: its integer part selects the current cycle number and
/// the fractional part selects the position within that cycle. Space-separated
/// values divide the cycle into equal time slots.
///
/// ## Values
///
/// | Syntax | Meaning | Example |
/// |--------|---------|---------|
/// | Note name | Pitch (octave defaults to 3) | `'c4'`, `'a#3'`, `'db5'` |
/// | Bare number | MIDI note number | `60`, `72` |
/// | `Xhz` | Frequency | `'440hz'` |
/// | `Xv` | Explicit voltage | `'0v'`, `'1v'`, `'-0.5v'` |
/// | `~` | Rest (gate low, no change in CV) | `'c4 ~ e4 ~'` |
///
/// Bare numbers are MIDI note numbers (A0 = MIDI 33 = 0 V).
///
/// ## Grouping
///
/// - **`[a b c]`** — fast subsequence: subdivides the parent time slot so all
///   elements play within it.
/// - **`<a b c>`** — slow / alternating: plays one element per cycle,
///   advancing each time the pattern loops.
///
/// ```js
/// $cycle("c4 [d4 e4]")   // c4 for half the cycle, d4 & e4 share the other half
/// $cycle("<c4 g4> e4")   // cycle 1: c4 e4, cycle 2: g4 e4, …
/// ```
///
/// ## Stacks
///
/// **`a b, c d`** — comma-separated patterns play **simultaneously** (layered).
/// Each sub-pattern has its own independent timing.
///
/// ```js
/// $cycle("c4 e4, g4 b4")   // two patterns layered on top of each other
/// $cycle("c4 d4 e4, g3")   // three-note melody over a pedal tone
/// ```
///
/// ## Random choice
///
/// **`a|b|c`** — randomly selects one option each time the slot is reached.
///
/// ```js
/// $cycle("c4|d4|e4 g4")  // first slot is a random pick each cycle
/// ```
///
/// ## Nesting
///
/// Grouping, stacks, and random choice nest arbitrarily:
///
/// ```js
/// $cycle("<c4 [d4 e4]> [f4|g4 a4]")  // slow + fast + random combined
/// $cycle("[c4 e4, g4] a4")            // stack inside a fast subsequence
/// ```
///
/// ## Per-element modifiers
///
/// Modifiers attach directly to an element (no spaces). Multiple modifiers
/// can be chained in any order.
///
/// | Modifier | Syntax | Meaning |
/// |----------|--------|---------|
/// | Weight | `@n` | Relative duration within a sequence (default 1). `c4@2 e4` gives c4 twice the time. |
/// | Speed up | `*n` | Repeat/subdivide `n` times within the slot. `c4*3` plays c4 three times. |
/// | Slow down | `/n` | Stretch over `n` cycles. `c4/2` plays every other cycle. |
/// | Replicate | `!n` | Duplicate the element `n` times (default 2). `c4!3` is equivalent to `c4 c4 c4`. |
/// | Degrade | `?` or `?n` | Randomly drop the element. `c4?` drops ~50 % of the time; `c4?0.8` drops 80 %. |
/// | Euclidean | `(k,n)` or `(k,n,offset)` | Distribute `k` pulses over `n` steps using the Bjorklund algorithm. Optional `offset` rotates the pattern. |
///
/// ```js
/// $cycle("c4*2 e4 g4")        // c4 plays twice in its slot
/// $cycle("c4@3 e4 g4")        // c4 gets 3/5 of the cycle, e4 and g4 get 1/5 each
/// $cycle("c4? e4 g4")         // c4 randomly drops out ~50 % of the time
/// $cycle("c4(3,8) e4")        // Euclidean: 3 hits spread over 8 steps
/// $cycle("[c4 d4 e4 f4](3,8)") // Euclidean applied to a subpattern
/// ```
///
/// Modifier operands can also be subpatterns: `c4*[2 3]` alternates between
/// doubling and tripling each slot.
///
/// ## Outputs
///
/// - **cv** — V/Oct pitch (C4 = 0 V).
/// - **gate** — 5 V while a note is active, 0 V otherwise.
/// - **trig** — single-sample 5 V pulse at each note onset.
#[module(
    name = "$cycle",
    channels_derive = seq_derive_channel_count,
    args(pattern),
    stateful,
)]
pub struct Seq {
    outputs: SeqOutputs,
    params: SeqParams,
    state: SeqState,
}

/// State for the Seq module.
struct SeqState {
    /// Per-voice state array
    voices: [VoiceState; PORT_MAX_CHANNELS],
    /// Round-robin voice index for allocation
    next_voice: usize,
    /// Last CV voltage per channel — holds through rest periods and state transfers
    last_cv: [f32; PORT_MAX_CHANNELS],
    /// Scratch buffer for voice release — reused each frame to avoid heap alloc
    voices_to_release: ArrayVec<usize, PORT_MAX_CHANNELS>,
    /// Scratch buffer for onset events awaiting voice allocation.
    events_to_process: ArrayVec<PendingEvent, PORT_MAX_CHANNELS>,
}

impl Default for SeqState {
    fn default() -> Self {
        Self {
            voices: std::array::from_fn(|_| VoiceState::default()),
            next_voice: 0,
            last_cv: [0.0; PORT_MAX_CHANNELS],
            voices_to_release: ArrayVec::new(),
            events_to_process: ArrayVec::new(),
        }
    }
}

impl Seq {
    /// Look up the storage for `cycle` in the baked ribbon window.
    fn get_cycle_storage(&self, cycle: i64) -> Option<&SeqCycleStorage> {
        super::cache::get_cycle_storage(
            cycle,
            self.params.ribbon.0,
            self.params.pattern.cached_haps(),
        )
    }

    fn update(&mut self, sample_rate: f32) {
        // `raw` is the monotonic clock playhead (cycles, fractional). The
        // ribbon folds it into the baked window with an unsigned modulo:
        // clock cycle 0 maps to baked cycle `offset`, and the window loops
        // forever, phase-locked to the clock.
        let raw = self.params.playhead.value_or_zero() as f64;
        let hold = min_gate_samples(sample_rate);
        let num_channels = self.channel_count();

        let (offset, length) = self.params.ribbon; // length > 0 (validated)
        // Fold the clock into the baked window. Clamp once for the window math
        // so a (deliberately wired) negative playhead pins to the cycle start
        // rather than producing a spurious mid-cycle phase. Release below keys
        // off the unclamped monotonic `raw`, tracking each note's true span.
        let raw_clamped = raw.max(0.0);
        let raw_cycle = raw_clamped.floor() as u64;
        let slot = raw_cycle % length; // 0..length
        let current_cycle = (offset + slot) as i64; // baked cycle ∈ [offset, offset+length)
        // Playhead in the baked cycle's logical frame — the same absolute
        // frame the cycle's haps live in, so onset / `already_assigned`
        // checks work exactly as before.
        let logical = current_cycle as f64 + (raw_clamped - raw_clamped.floor());

        // Release voices whose notes have ended. A note's life is tracked in
        // the monotonic `raw` frame (two-sided, so a backward scrub also
        // frees), because `logical` wraps at the ribbon seam.
        self.state.voices_to_release.clear();
        for i in 0..num_channels {
            if let Some(cached) = self.state.voices[i].cached_hap {
                if !cached.contains(raw) {
                    self.state.voices_to_release.push(i);
                }
            }
        }
        for i in self.state.voices_to_release.iter().copied() {
            self.state.voices[i].active = false;
            self.state.voices[i].cached_hap = None;
            self.state.voices[i]
                .gate
                .set_state(TempGateState::Low, TempGateState::Low, 0);
        }

        if self.params.pattern.pattern().is_none() {
            for ch in 0..num_channels {
                self.outputs.cv.set(ch, 0.0);
                self.outputs
                    .gate
                    .set(ch, self.state.voices[ch].gate.process());
                self.outputs
                    .trig
                    .set(ch, self.state.voices[ch].trigger.process());
            }
            return;
        }

        // Collect new onsets for this frame, then dispatch to voices in a
        // separate pass so we don't hold a borrow of cycle storage while
        // mutating voice state.
        let SeqState {
            voices,
            events_to_process,
            ..
        } = &mut self.state;
        events_to_process.clear();
        let storage_opt = self.params.pattern.cached_haps().get(slot as usize);
        if let Some(storage) = storage_opt {
            for (hap_index, hap) in storage.haps.iter().enumerate() {
                if !hap.has_onset || logical < hap.part_begin || logical >= hap.part_end {
                    continue;
                }
                if hap.value.is_rest() {
                    continue;
                }
                let hap_index = hap_index as u32;
                let already_assigned = (0..num_channels).any(|i| {
                    voices[i]
                        .cached_hap
                        .map(|existing| {
                            existing.hap_index == hap_index
                                && existing.cached_cycle == current_cycle
                        })
                        .unwrap_or(false)
                });
                if already_assigned {
                    continue;
                }
                if events_to_process.remaining_capacity() == 0 {
                    break;
                }
                events_to_process.push(PendingEvent {
                    hap_index,
                    whole_begin: hap.whole_begin,
                    whole_end: hap.whole_end,
                    value: hap.value,
                });
            }
        }

        let state = &mut self.state;
        for event in state.events_to_process.iter().copied() {
            let mut found = None;
            for i in 0..num_channels {
                let idx = (state.next_voice + i) % num_channels;
                if !state.voices[idx].active {
                    state.next_voice = (idx + 1) % num_channels;
                    state.voices[idx].last_assigned = raw;
                    found = Some(idx);
                    break;
                }
            }
            let Some(voice_idx) = found else { continue };
            let voice = &mut state.voices[voice_idx];
            // Map the note's logical span onto the current absolute lap so
            // release keys off the monotonic `raw` clock and the note plays
            // its full length even across the ribbon wrap.
            voice.cached_hap = Some(CachedHap {
                hap_index: event.hap_index,
                cached_cycle: current_cycle,
                whole_begin: event.whole_begin,
                whole_end: event.whole_end,
                raw_begin: raw - (logical - event.whole_begin),
                raw_end: raw + (event.whole_end - logical),
                value: event.value,
            });
            voice.active = true;
            voice
                .gate
                .set_state(TempGateState::Low, TempGateState::High, hold);
            voice
                .trigger
                .set_state(TempGateState::High, TempGateState::Low, hold);
        }

        for ch in 0..num_channels {
            let voice = &mut state.voices[ch];
            if let Some(cached) = voice.cached_hap
                && let Some(cv) = cached.get_cv()
            {
                state.last_cv[ch] = cv as f32;
            }
            self.outputs.cv.set(ch, state.last_cv[ch]);
            self.outputs.gate.set(ch, voice.gate.process());
            self.outputs.trig.set(ch, voice.trigger.process());
        }
    }
}

/// Onset event awaiting voice allocation. Scalars only — no heap reference.
#[derive(Clone, Copy, Debug)]
struct PendingEvent {
    hap_index: u32,
    whole_begin: f64,
    whole_end: f64,
    value: SeqValue,
}

/// Resolve the hap in `storage` that the voice's cached scalars describe.
///
/// The voice's `cached.hap_index` is frozen at onset time against whatever
/// cache geometry was current then. A live patch re-run can `std::mem::swap`
/// an old `SeqState` into a freshly-baked module whose `get_state` reads a
/// re-built cache; the cached index then misses or points at a hap with
/// different geometry. This read-only resolver trusts `cached.hap_index`
/// only when it is in range AND its `whole_begin`/`whole_end` match the
/// voice's held scalars; otherwise it linear-scans the cycle's haps for the
/// matching geometry. `cached.hap_index` is owned by the audio thread and is
/// never written here.
fn resolve_hap_index(storage: &SeqCycleStorage, cached: &CachedHap) -> Option<usize> {
    const EPS: f64 = 1e-6;
    let geometry_matches = |hap: &super::seq_value::SeqCycleHap| {
        (hap.whole_begin - cached.whole_begin).abs() < EPS
            && (hap.whole_end - cached.whole_end).abs() < EPS
    };
    // Held value disambiguates two simultaneous same-geometry haps (a chord /
    // stacked source) that would otherwise alias onto the same index. It is a
    // tie-breaker, not a requirement: a live state transfer onto a pattern
    // with different values must still re-resolve by geometry alone.
    let value_matches = |value: &SeqValue| match (value, &cached.value) {
        (SeqValue::Voltage(a), SeqValue::Voltage(b)) => (a - b).abs() < EPS,
        (SeqValue::Rest, SeqValue::Rest) => true,
        _ => false,
    };
    let idx = cached.hap_index as usize;
    if let Some(hap) = storage.haps.get(idx)
        && geometry_matches(hap)
        && value_matches(&hap.value)
    {
        return Some(idx);
    }
    storage
        .haps
        .iter()
        .position(|h| geometry_matches(h) && value_matches(&h.value))
        .or_else(|| storage.haps.iter().position(geometry_matches))
}

impl crate::types::StatefulModule for Seq {
    fn get_state(&self) -> Option<serde_json::Value> {
        let num_channels = self.channel_count().clamp(1, PORT_MAX_CHANNELS);
        let per_source = self.params.pattern.per_source();
        let num_sources = per_source.len().max(1);

        // Per-pattern active spans from all active voices.
        let mut per_pattern_spans: Vec<Vec<(usize, usize)>> = vec![Vec::new(); num_sources];
        let mut any_non_rest = false;

        for voice in self.state.voices.iter().take(num_channels) {
            if let Some(cached) = voice.cached_hap
                && !cached.is_rest()
            {
                any_non_rest = true;
                if let Some(storage) = self.get_cycle_storage(cached.cached_cycle)
                    && let Some(hap_index) = resolve_hap_index(storage, &cached)
                    && let Some(hap) = storage.haps.get(hap_index)
                {
                    let start = hap.span_offset as usize;
                    let end = start + hap.span_len as usize;
                    for span in &storage.span_arena[start..end] {
                        let idx = span.pattern_idx as usize;
                        if idx < num_sources {
                            per_pattern_spans[idx].push((span.start as usize, span.end as usize));
                        }
                    }
                }
            }
        }

        if !any_non_rest && per_pattern_spans.iter().all(|s| s.is_empty()) {
            None
        } else {
            for spans in &mut per_pattern_spans {
                spans.sort();
                spans.dedup();
            }

            // Build param_spans keyed by "pattern" for single-source legacy
            // payloads and "pattern.0", "pattern.1", ... for chained `$p.s`
            // payloads. The argument-span analyzer registers chain RHS
            // literals under the chain call site, so the renderer maps
            // pattern_idx > 0 to those.
            let is_multi = self.params.pattern.is_multi_source();
            let mut param_spans = serde_json::Map::new();
            if !is_multi
                && num_sources == 1
                && let Some(meta) = per_source.first()
            {
                param_spans.insert(
                    "pattern".to_string(),
                    serde_json::json!({
                        "spans": &per_pattern_spans[0],
                        "source": meta.source,
                        "all_spans": meta.all_spans,
                    }),
                );
            } else {
                for (i, meta) in per_source.iter().enumerate() {
                    param_spans.insert(
                        format!("pattern.{i}"),
                        serde_json::json!({
                            "spans": per_pattern_spans
                                .get(i)
                                .cloned()
                                .unwrap_or_default(),
                            "source": meta.source,
                            "all_spans": meta.all_spans,
                        }),
                    );
                }
            }

            Some(serde_json::json!({
                "param_spans": param_spans,
                "num_channels": num_channels,
            }))
        }
    }
}

message_handlers!(impl Seq {});
