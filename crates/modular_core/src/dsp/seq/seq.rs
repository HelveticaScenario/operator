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
}

impl Default for VoiceState {
    fn default() -> Self {
        Self {
            cached_hap: None,
            gate: TempGate::new_gate(TempGateState::Low),
            trigger: TempGate::new_gate(TempGateState::Low),
            active: false,
        }
    }
}

fn default_channels() -> usize {
    4
}

/// Default ribbon loop length in cycles. Same memory/cost as the old
/// param cache (cycles `0..1024` baked at parse time).
const DEFAULT_RIBBON_LENGTH: f64 = 1024.0;

/// Largest ribbon loop length. The window touches `ceil(offset+length) -
/// floor(offset)` integer cycles, each baked synchronously on the main thread
/// on every pattern/ribbon edit, so it is capped.
const MAX_RIBBON_LENGTH: f64 = 8192.0;

/// Largest ribbon offset. Generous, and keeps `offset + length` exact in
/// `f64` (well under 2^53).
const MAX_RIBBON_OFFSET: f64 = 1_000_000.0;

fn default_ribbon() -> (f64, f64) {
    (0.0, DEFAULT_RIBBON_LENGTH)
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
    /// Number of polyphonic voices (1-64)
    #[deserr(default)]
    pub channels: Option<usize>,
    /// loop window [offset, length] in cycles (fractional allowed)
    #[serde(default = "default_ribbon")]
    #[deserr(default = default_ribbon())]
    ribbon: (f64, f64),
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
    let reject = |msg: String| {
        let mut err = ModuleParamErrors::default();
        err.add("ribbon".to_string(), msg);
        Err(err)
    };
    // Finiteness first, so ±∞ reports as non-finite rather than over-cap and
    // NaN never reaches the comparisons below (where it would silently pass).
    if !offset.is_finite() || !length.is_finite() {
        return reject("ribbon values must be finite".to_string());
    }
    if length <= 0.0 {
        return reject("ribbon loop length must be greater than 0".to_string());
    }
    if length > MAX_RIBBON_LENGTH {
        return reject(format!(
            "ribbon loop length must be {MAX_RIBBON_LENGTH} cycles or fewer"
        ));
    }
    if offset < 0.0 {
        return reject("ribbon offset must be 0 or greater".to_string());
    }
    if offset > MAX_RIBBON_OFFSET {
        return reject(format!(
            "ribbon offset must be {MAX_RIBBON_OFFSET} cycles or fewer"
        ));
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
/// $cycle($p("c4 [d4 e4]"))   // c4 for half the cycle, d4 & e4 share the other half
/// $cycle($p("<c4 g4> e4"))   // cycle 1: c4 e4, cycle 2: g4 e4, …
/// ```
///
/// ## Stacks
///
/// **`a b, c d`** — comma-separated patterns play **simultaneously** (layered).
/// Each sub-pattern has its own independent timing.
///
/// ```js
/// $cycle($p("c4 e4, g4 b4"))   // two patterns layered on top of each other
/// $cycle($p("c4 d4 e4, g3"))   // three-note melody over a pedal tone
/// ```
///
/// ## Random choice
///
/// **`a|b|c`** — randomly selects one option each time the slot is reached.
///
/// ```js
/// $cycle($p("c4|d4|e4 g4"))  // first slot is a random pick each cycle
/// ```
///
/// ## Nesting
///
/// Grouping, stacks, and random choice nest arbitrarily:
///
/// ```js
/// $cycle($p("<c4 [d4 e4]> [f4|g4 a4]"))  // slow + fast + random combined
/// $cycle($p("[c4 e4, g4] a4"))           // stack inside a fast subsequence
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
/// $cycle($p("c4*2 e4 g4"))        // c4 plays twice in its slot
/// $cycle($p("c4@3 e4 g4"))        // c4 gets 3/5 of the cycle, e4 and g4 get 1/5 each
/// $cycle($p("c4? e4 g4"))         // c4 randomly drops out ~50 % of the time
/// $cycle($p("c4(3,8) e4"))        // Euclidean: 3 hits spread over 8 steps
/// $cycle($p("[c4 d4 e4 f4](3,8)")) // Euclidean applied to a subpattern
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
    /// Per-channel voice state, one element per polyphonic channel. Sized to the
    /// derived channel count by the `#[module]` macro and carried across patch
    /// updates by the macro's per-element state transfer.
    channel_state: Box<[SeqChannel]>,
}

/// Per-channel sequencer state.
#[derive(Default)]
struct SeqChannel {
    /// Per-voice playback state.
    voice: VoiceState,
    /// Last CV voltage — holds through rest periods and state transfers. Also
    /// the "value the voice held before", used for nearest-value allocation.
    last_cv: f32,
}

/// Module-level state for the Seq module.
struct SeqState {
    /// Previous frame's folded playhead position, for window-membership edge
    /// detection: a hap fires only when the playhead *enters* its window (an
    /// outside→inside transition), never because it is merely inside it.
    prev_logical: f64,
    /// Previous frame's clamped raw playhead, for lap detection: when the raw
    /// playhead crosses a ribbon seam, the window replays from the top, so the
    /// edge check must fire even a hap whose part contained `prev_logical`
    /// (e.g. one covering the entire window).
    prev_raw: f64,
    /// False until the first `update`. On the first frame there is no previous
    /// position, so every in-window onset fires (a fresh `$cycle` starts playing
    /// from the current position).
    started: bool,
    /// Scratch buffer for voice release — reused each frame to avoid heap alloc
    voices_to_release: ArrayVec<usize, PORT_MAX_CHANNELS>,
    /// Scratch buffer for onset events awaiting voice allocation.
    events_to_process: ArrayVec<PendingEvent, PORT_MAX_CHANNELS>,
    /// Pre-allocated DP scratch for nearest-value voice assignment. Boxed and
    /// built here (main thread) so the audio thread never allocates.
    assign: Box<AssignScratch>,
}

impl Default for SeqState {
    fn default() -> Self {
        Self {
            prev_logical: 0.0,
            prev_raw: 0.0,
            started: false,
            voices_to_release: ArrayVec::new(),
            events_to_process: ArrayVec::new(),
            assign: Box::new(AssignScratch::default()),
        }
    }
}

impl Seq {
    /// Look up the storage for `cycle` in the baked ribbon window.
    fn get_cycle_storage(&self, cycle: i64) -> Option<&SeqCycleStorage> {
        super::cache::get_cycle_storage(
            cycle,
            self.params.ribbon.0.floor() as i64,
            self.params.pattern.cached_haps(),
        )
    }

    /// True when the voice's held note no longer exists in the *current* baked
    /// pattern, matched on BOTH time-window AND value. Such a voice is a lingering
    /// old-pattern note and may be stolen for a new onset; a voice that still has
    /// an exact window+value match is a genuine continuation and is never stolen.
    ///
    /// Both keys are required. Window-only would wrongly keep an old `c4[0,1]`
    /// when the new pattern only has `e5[0,1]` at that geometry; value-only would
    /// wrongly keep it when the new pattern has `c4` at a *different* window (e.g.
    /// `[c4*2]`). Note: this is stricter than `resolve_hap_index`, whose
    /// geometry-only fallback is for highlight display, not steal eligibility.
    fn voice_is_orphan(&self, cached: &CachedHap) -> bool {
        const EPS: f64 = 1e-6;
        let Some(storage) = self.get_cycle_storage(cached.cached_cycle) else {
            return true; // cycle no longer in the baked window → orphaned
        };
        !storage.haps.iter().any(|h| {
            (h.whole_begin - cached.whole_begin).abs() < EPS
                && (h.whole_end - cached.whole_end).abs() < EPS
                && match (h.value, cached.value) {
                    (SeqValue::Voltage(a), SeqValue::Voltage(b)) => (a - b).abs() < EPS,
                    (SeqValue::Rest, SeqValue::Rest) => true,
                    _ => false,
                }
        })
    }

    fn update(&mut self, sample_rate: f32) {
        // `raw` is the monotonic clock playhead (cycles, fractional). The
        // ribbon folds it into the baked window with a continuous-time modulo,
        // so a fractional `offset`/`length` defines a loop window whose seam
        // can fall mid-cycle. The window loops forever, phase-locked to the
        // clock: clock pos 0 plays the window start (`offset`).
        let raw = self.params.playhead.value_or_zero() as f64;
        let hold = min_gate_samples(sample_rate);
        let num_channels = self.channel_count();

        let (offset, length) = self.params.ribbon; // length > 0 (validated)
        // Fold the clock into the baked window. Clamp once for the window math
        // so a (deliberately wired) negative playhead pins to the window start
        // rather than producing a spurious mid-cycle phase. Release below keys
        // off the unclamped monotonic `raw`, tracking each note's true span.
        let raw_clamped = raw.max(0.0);
        let base = offset.floor() as i64; // first baked cycle = floor(offset)
        let phase = raw_clamped.rem_euclid(length); // [0, length)
        let pos = offset + phase; // playhead in the window, ∈ [offset, offset+length)
        // Integer cycle for the storage lookup and the `already_assigned`
        // dedup key. `pos.floor() >= base`, so the index below is never
        // negative.
        let current_cycle = pos.floor() as i64;
        // Playhead in the pattern's absolute frame — the same frame the baked
        // cycle's haps live in, so onset / `already_assigned` checks work
        // exactly as before. (For integer offset/length, `pos == old logical`.)
        let logical = pos;
        let storage_index = (current_cycle - base) as usize;

        // Window-membership edge detection baseline: capture last frame's folded
        // position, then advance it for this frame (unconditionally, so the
        // baseline stays correct even on the no-pattern early-return below). A
        // hap fires only on an outside→inside transition (see the collection
        // pass), so a mid-window pattern swap or scrub does not re-trigger it.
        let prev_logical = self.state.prev_logical;
        let started = self.state.started;
        self.state.prev_logical = logical;
        self.state.started = true;

        // Forward seam crossing: the raw playhead moved onto a new lap of the
        // window, so the whole window replays and in-window onsets fire again.
        // Both lap indices use the current `length`, so a ribbon edit cannot
        // fabricate a crossing on its own.
        let prev_raw = self.state.prev_raw;
        self.state.prev_raw = raw_clamped;
        let lap_wrapped = (raw_clamped / length).floor() > (prev_raw / length).floor();

        // Release voices whose notes have ended. A note's life is tracked in
        // the monotonic `raw` frame (two-sided, so a backward scrub also
        // frees), because `logical` wraps at the ribbon seam.
        self.state.voices_to_release.clear();
        for i in 0..num_channels {
            if let Some(cached) = self.channel_state[i].voice.cached_hap {
                if !cached.contains(raw) {
                    self.state.voices_to_release.push(i);
                }
            }
        }
        for i in self.state.voices_to_release.iter().copied() {
            self.channel_state[i].voice.active = false;
            self.channel_state[i].voice.cached_hap = None;
            self.channel_state[i]
                .voice
                .gate
                .set_state(TempGateState::Low, TempGateState::Low, 0);
        }

        if self.params.pattern.pattern().is_none() {
            for ch in 0..num_channels {
                self.outputs.cv.set(ch, 0.0);
                self.outputs
                    .gate
                    .set(ch, self.channel_state[ch].voice.gate.process());
                self.outputs
                    .trig
                    .set(ch, self.channel_state[ch].voice.trigger.process());
            }
            return;
        }

        // Collect new onsets for this frame, then dispatch to voices in a
        // separate pass so we don't hold a borrow of cycle storage while
        // mutating voice state.
        let events_to_process = &mut self.state.events_to_process;
        events_to_process.clear();
        let storage_opt = self.params.pattern.cached_haps().get(storage_index);
        if let Some(storage) = storage_opt {
            for (hap_index, hap) in storage.haps.iter().enumerate() {
                if !hap.has_onset || logical < hap.part_begin || logical >= hap.part_end {
                    continue;
                }
                if hap.value.is_rest() {
                    continue;
                }
                // Window-membership edge: fire only on *entering* the window. Skip
                // if the playhead was already inside it last frame — a mid-window
                // pattern swap, a scrub that lands inside, or a paused playhead is
                // not an onset. (`started` is false on the very first frame, so a
                // fresh module fires its in-window notes.) A seam crossing resets
                // the edge: a hap whose part spans the entire window re-fires once
                // per lap; a note still sounding across the seam stays suppressed
                // by the `already_assigned` dedup below.
                if started
                    && !lap_wrapped
                    && prev_logical >= hap.part_begin
                    && prev_logical < hap.part_end
                {
                    continue;
                }
                let hap_index = hap_index as u32;
                let already_assigned = (0..num_channels).any(|i| {
                    self.channel_state[i]
                        .voice
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

        // Assign this frame's onsets to voices by nearest previously-held value
        // (voice leading). Tier 1 draws from the free (inactive) voices; only if
        // they run out does Tier 2 steal "orphaned" voices — old-pattern notes
        // that no longer exist in the current pattern — so a new hap is never
        // dropped while a stale note rings. A current-pattern continuation is
        // never stolen. Onsets with no free or orphan voice are dropped.
        let n = self.state.events_to_process.len();
        if n > 0 {
            let mut assigned: [Option<usize>; PORT_MAX_CHANNELS] = [None; PORT_MAX_CHANNELS];

            // Tier 1: free voices.
            let mut free: ArrayVec<usize, PORT_MAX_CHANNELS> = ArrayVec::new();
            for i in 0..num_channels {
                if !self.channel_state[i].voice.active {
                    free.push(i);
                }
            }
            {
                let SeqState {
                    events_to_process,
                    assign,
                    ..
                } = &mut self.state;
                assign_nearest(
                    events_to_process,
                    &mut assigned,
                    &free,
                    &self.channel_state,
                    assign,
                );
            }

            // Tier 2: steal orphaned voices, computed lazily only when onsets
            // remain unplaced (the steady state never reaches here).
            if (0..n).any(|ei| assigned[ei].is_none()) {
                let mut orphans: ArrayVec<usize, PORT_MAX_CHANNELS> = ArrayVec::new();
                for i in 0..num_channels {
                    if self.channel_state[i].voice.active
                        && let Some(cached) = self.channel_state[i].voice.cached_hap
                        && self.voice_is_orphan(&cached)
                    {
                        orphans.push(i);
                    }
                }
                let SeqState {
                    events_to_process,
                    assign,
                    ..
                } = &mut self.state;
                assign_nearest(
                    events_to_process,
                    &mut assigned,
                    &orphans,
                    &self.channel_state,
                    assign,
                );
            }

            // Apply the chosen voice for each placed onset. Map the note's logical
            // span onto the current absolute lap so release keys off the monotonic
            // `raw` clock and the note plays its full length across the ribbon
            // wrap. A stolen voice re-triggers (gate Low→High), cutting its old note.
            for ei in 0..n {
                let Some(voice_idx) = assigned[ei] else {
                    continue;
                };
                let event = self.state.events_to_process[ei];
                let voice = &mut self.channel_state[voice_idx].voice;
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
        }

        for ch in 0..num_channels {
            let channel = &mut self.channel_state[ch];
            if let Some(cached) = channel.voice.cached_hap
                && let Some(cv) = cached.get_cv()
            {
                channel.last_cv = cv as f32;
            }
            self.outputs.cv.set(ch, channel.last_cv);
            self.outputs.gate.set(ch, channel.voice.gate.process());
            self.outputs.trig.set(ch, channel.voice.trigger.process());
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

/// Square dimension of the assignment DP table: one extra row/column for the
/// empty prefix. Scales with `PORT_MAX_CHANNELS`, so a future channel bump
/// resizes the scratch automatically.
const DP_DIM: usize = PORT_MAX_CHANNELS + 1;

/// Pre-allocated scratch for the voice-assignment DP ([`assign_nearest`]). One
/// decision bit per cell; boxed in `SeqState` and built on the main thread so
/// the audio thread never allocates.
struct AssignScratch {
    /// `decision[i * DP_DIM + j]` records the choice at cell `(i, j)`:
    /// `true` = match row item `i-1` to column item `j-1`; `false` = skip
    /// column `j-1`. Only the cells visited by the current call are written
    /// before they are read back, so the table needs no per-call clear.
    decision: [bool; DP_DIM * DP_DIM],
}

impl Default for AssignScratch {
    fn default() -> Self {
        Self {
            decision: [false; DP_DIM * DP_DIM],
        }
    }
}

/// Assign the still-unplaced onsets in `events` to `candidates` voices so that
/// total pitch movement `Σ |onset voltage − last_cv[voice]|` is minimized — the
/// provably-optimal voice leading, not a greedy approximation.
///
/// Because the cost is `|Δvalue|` on a line, the optimal matching is
/// order-preserving: sort both sides by value and match them with a 1-D DP
/// (`dp[i][j] = min(skip column j, match row i to column j + cost)`). The
/// shorter side is fully matched; the longer side contributes a chosen subset.
/// Runs in `O(E·V)` (plus an `O(N log N)` in-place sort), versus the cubic of a
/// repeated global-nearest scan — cheap even at the max channel count.
///
/// Onsets already placed (by an earlier tier) are skipped, so this is called
/// once per tier (free voices, then orphaned voices). Allocation-free: fixed
/// `[_; PORT_MAX_CHANNELS]` stacks, an in-place `sort_unstable`, and the boxed
/// `scratch` decision table. Each candidate's previously-held value is read
/// from `channel_state[voice].last_cv`.
fn assign_nearest(
    events: &[PendingEvent],
    assigned: &mut [Option<usize>; PORT_MAX_CHANNELS],
    candidates: &[usize],
    channel_state: &[SeqChannel],
    scratch: &mut AssignScratch,
) {
    // Gather the unplaced onsets and the candidate voices as (value, index)
    // pairs. Index is the tie-break so the comparator is a total order, making
    // the allocation-free unstable sort deterministic.
    let mut evs: ArrayVec<(f32, u32), PORT_MAX_CHANNELS> = ArrayVec::new();
    for (ei, ev) in events.iter().enumerate() {
        if assigned[ei].is_none() {
            evs.push((ev.value.to_voltage().unwrap_or(0.0) as f32, ei as u32));
        }
    }
    let mut cands: ArrayVec<(f32, u32), PORT_MAX_CHANNELS> = ArrayVec::new();
    for &vidx in candidates {
        cands.push((channel_state[vidx].last_cv, vidx as u32));
    }
    if evs.is_empty() || cands.is_empty() {
        return;
    }
    let by_value = |a: &(f32, u32), b: &(f32, u32)| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(Ordering::Equal)
            .then(a.1.cmp(&b.1))
    };
    evs.sort_unstable_by(by_value);
    cands.sort_unstable_by(by_value);

    // Rows = shorter side (every item matched); cols = longer side (a subset is
    // chosen). Whether events or candidates are the rows flips which index in a
    // matched pair is the event vs the voice.
    let events_are_rows = evs.len() <= cands.len();
    let (rows, cols): (&[(f32, u32)], &[(f32, u32)]) = if events_are_rows {
        (&evs, &cands)
    } else {
        (&cands, &evs)
    };
    let s = rows.len();
    let l = cols.len();

    // Order-preserving min-cost DP, rolled to two rows. `dp[j]` is the cost of
    // matching the first `i` row items into the first `j` columns; +∞ marks an
    // infeasible prefix (fewer columns than rows).
    let mut dp_prev = [f32::INFINITY; DP_DIM];
    let mut dp_cur = [f32::INFINITY; DP_DIM];
    for slot in dp_prev.iter_mut().take(l + 1) {
        *slot = 0.0; // row 0: nothing matched yet, cost 0 for any column count
    }
    for i in 1..=s {
        dp_cur[0] = f32::INFINITY; // can't match ≥1 items into 0 columns
        for j in 1..=l {
            let skip = dp_cur[j - 1];
            let take = if dp_prev[j - 1].is_finite() {
                dp_prev[j - 1] + (rows[i - 1].0 - cols[j - 1].0).abs()
            } else {
                f32::INFINITY
            };
            // Prefer skip on ties → the lowest-index (after sort) column wins,
            // matching the old lowest-voice-index tie-break.
            if take < skip {
                dp_cur[j] = take;
                scratch.decision[i * DP_DIM + j] = true;
            } else {
                dp_cur[j] = skip;
                scratch.decision[i * DP_DIM + j] = false;
            }
        }
        std::mem::swap(&mut dp_prev, &mut dp_cur);
    }

    // Backtrack from (s, l), recording matched pairs into `assigned`.
    let mut i = s;
    let mut j = l;
    while i > 0 && j > 0 {
        if scratch.decision[i * DP_DIM + j] {
            let row_idx = rows[i - 1].1 as usize;
            let col_idx = cols[j - 1].1 as usize;
            let (event_index, voice_index) = if events_are_rows {
                (row_idx, col_idx)
            } else {
                (col_idx, row_idx)
            };
            assigned[event_index] = Some(voice_index);
            i -= 1;
            j -= 1;
        } else {
            j -= 1;
        }
    }
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
    /// Write the currently-highlighted step spans into `out` on the audio thread,
    /// without allocating. The parts that don't change while playing (`source`,
    /// `all_spans`, `argument_spans`) live on the main thread and are merged with
    /// this snapshot on poll (see [`crate::dsp::seq::highlight`]).
    ///
    /// Spans are stored by source index; the main thread maps index `i` to the
    /// editor key `"pattern"` or `"pattern.{i}"` from the same params.
    fn write_module_state(&self, out: &mut dyn crate::module_state::ModuleLiveState) {
        let Some(out) = out
            .as_any_mut()
            .downcast_mut::<crate::dsp::seq::SeqHighlightState>()
        else {
            return;
        };
        out.reset();
        let num_channels = self.channel_count().clamp(1, PORT_MAX_CHANNELS);
        let num_sources = self.params.pattern.per_source().len().max(1);

        for channel in self.channel_state.iter().take(num_channels) {
            let voice = &channel.voice;
            if let Some(cached) = voice.cached_hap
                && !cached.is_rest()
                && let Some(storage) = self.get_cycle_storage(cached.cached_cycle)
                && let Some(hap_index) = resolve_hap_index(storage, &cached)
                && let Some(hap) = storage.haps.get(hap_index)
            {
                let start = hap.span_offset as usize;
                let end = start + hap.span_len as usize;
                for span in &storage.span_arena[start..end] {
                    let idx = span.pattern_idx as usize;
                    if idx < num_sources {
                        out.push_span(idx, span.start as u32, span.end as u32);
                    }
                }
            }
        }
    }
}

message_handlers!(impl Seq {});

#[cfg(test)]
mod tests {
    use super::*;

    fn params_with_ribbon(ribbon: (f64, f64)) -> SeqParams {
        SeqParams {
            pattern: SeqPatternParam::default(),
            playhead: None,
            channels: None,
            ribbon,
            pattern_source: String::new(),
        }
    }

    fn reject_message(ribbon: (f64, f64)) -> Option<String> {
        seq_bake_ribbon(params_with_ribbon(ribbon), deserr::ValuePointerRef::Origin)
            .err()
            .map(|e| e.to_string())
    }

    /// NaN / ±∞ cannot reach this hook through the JSON graph (serde_json
    /// rejects non-finite numbers), but the hook guards finiteness directly so
    /// no caller can ever drive `floor`/`ceil`/`rem_euclid` with a non-finite
    /// ribbon value. The finite check runs first, ahead of the magnitude
    /// bounds, so ±∞ is reported as non-finite rather than over-cap.
    #[test]
    fn seq_bake_ribbon_rejects_non_finite() {
        for bad in [
            (f64::NAN, 4.0),
            (0.0, f64::NAN),
            (f64::INFINITY, 4.0),
            (0.0, f64::INFINITY),
            (f64::NEG_INFINITY, 4.0),
            (0.0, f64::NEG_INFINITY),
        ] {
            let msg = reject_message(bad)
                .unwrap_or_else(|| panic!("expected rejection for ribbon {bad:?}"));
            assert!(
                msg.contains("ribbon values must be finite"),
                "ribbon {bad:?} got: {msg}"
            );
        }
    }

    #[test]
    fn seq_bake_ribbon_accepts_finite_fractional() {
        assert!(
            seq_bake_ribbon(
                params_with_ribbon((0.5, 1.5)),
                deserr::ValuePointerRef::Origin
            )
            .is_ok(),
            "a finite fractional ribbon must pass the bounds hook"
        );
    }

    fn ev(v: f64) -> PendingEvent {
        PendingEvent {
            hap_index: 0,
            whole_begin: 0.0,
            whole_end: 1.0,
            value: SeqValue::Voltage(v),
        }
    }

    /// Run `assign_nearest` over the given events and candidate voices (with the
    /// given `last_cv` for the referenced voices) and return the assignment.
    fn run_assign(
        events: &[PendingEvent],
        cands: &[usize],
        cv: &[(usize, f32)],
    ) -> Vec<Option<usize>> {
        let mut channels: Vec<SeqChannel> = (0..PORT_MAX_CHANNELS)
            .map(|_| SeqChannel::default())
            .collect();
        for &(i, v) in cv {
            channels[i].last_cv = v;
        }
        let mut assigned = [None; PORT_MAX_CHANNELS];
        let mut scratch = AssignScratch::default();
        assign_nearest(events, &mut assigned, cands, &channels, &mut scratch);
        assigned[..events.len()].to_vec()
    }

    #[test]
    fn assign_nearest_picks_optimal_subset_not_rank_match() {
        // Events 0 V and 10 V; three voices last held 0, 1, 10 V. The optimal
        // (min total movement) matching is 0→v0 and 10→v2 (cost 0). A naive
        // sorted rank-match (0→v0, 10→v1) would cost 9 — this guards that the DP
        // chooses *which* voices, not just pairs them in order.
        let got = run_assign(
            &[ev(0.0), ev(10.0)],
            &[0, 1, 2],
            &[(0, 0.0), (1, 1.0), (2, 10.0)],
        );
        assert_eq!(got[0], Some(0), "0 V → voice 0");
        assert_eq!(got[1], Some(2), "10 V → voice 2 (last_cv 10), not voice 1");
    }

    #[test]
    fn assign_nearest_drops_excess_onsets_optimally() {
        // Three events, two voices: place the two with zero movement (0→v0,
        // 10→v1) and drop the middle 5 V onset.
        let got = run_assign(
            &[ev(0.0), ev(5.0), ev(10.0)],
            &[0, 1],
            &[(0, 0.0), (1, 10.0)],
        );
        assert_eq!(got[0], Some(0));
        assert_eq!(got[2], Some(1));
        assert_eq!(got[1], None, "the 5 V event is dropped (only two voices)");
    }

    #[test]
    fn assign_nearest_minimizes_total_movement_three_voices() {
        // Voices last held 0, 1, 2 V; onsets at 2, 1, 0 V (reversed order). Each
        // onset must land on the voice already at its pitch (total movement 0),
        // regardless of event order.
        let got = run_assign(
            &[ev(2.0), ev(1.0), ev(0.0)],
            &[0, 1, 2],
            &[(0, 0.0), (1, 1.0), (2, 2.0)],
        );
        assert_eq!(got[0], Some(2), "2 V → voice 2");
        assert_eq!(got[1], Some(1), "1 V → voice 1");
        assert_eq!(got[2], Some(0), "0 V → voice 0");
    }

    #[test]
    fn assign_nearest_identical_values_use_distinct_voices() {
        // Two onsets at the same value with two equal-prior voices: both placed
        // on distinct voices (no collapse), deterministically by index.
        let got = run_assign(&[ev(0.0), ev(0.0)], &[0, 1], &[(0, 0.0), (1, 0.0)]);
        assert!(got[0].is_some() && got[1].is_some(), "both onsets placed");
        assert_ne!(got[0], got[1], "onsets take distinct voices");
    }
}
