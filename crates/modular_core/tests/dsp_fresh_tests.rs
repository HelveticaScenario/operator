//! Fresh integration tests for DSP modules.
//!
//! These tests verify that DSP modules produce correct audio output by
//! constructing modules via the public API, setting params as JSON, and
//! reading samples after ticking.

use std::collections::HashMap;

use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::DeserializedParams;
use modular_core::patch::Patch;
use modular_core::types::{ModuleSpec, PatchGraph, Sampleable};
use serde_json::{Value, json};

/// Helper — build a mini-notation payload shaped like what `$p(source)`
/// would emit on the DSL side. `$cycle` no longer accepts bare strings; the
/// wire shape is `{ ast, source, all_spans }`.
fn mini_payload(source: &str) -> Value {
    let parsed = modular_core::dsp::seq::seq_value::ParsedPatternPayload::parse_for_test(source);
    serde_json::to_value(&parsed).expect("payload should serialize")
}

const SAMPLE_RATE: f32 = 48000.0;
const DEFAULT_PORT: &str = "output";
/// Block size used at construction by every test in this file (direct-module
/// and patch-level). Bumping this exercises the wrapper's per-block dispatch
/// — the collect helpers walk all `TEST_BLOCK_SIZE` slots between
/// `start_block` calls.
///
/// Note: cycle classification (`Block` vs `Sample`) is the caller's
/// responsibility in `Patch::from_graph`; the tests below pass an empty
/// `mode_map`, which defaults every module to `Block`. Cycle-aware tests
/// would build the map via `modular::graph_analysis::analyze` first, but the
/// patches here are acyclic.
const TEST_BLOCK_SIZE: usize = 1;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Create a named module from the constructor registry with given params.
fn make_module(module_type: &str, id: &str, params: serde_json::Value) -> Box<dyn Sampleable> {
    let constructors = get_constructors();
    let deserializers = get_params_deserializers();
    let deserializer = deserializers
        .get(module_type)
        .unwrap_or_else(|| panic!("no params deserializer for '{module_type}'"));
    let cached = deserializer(params)
        .unwrap_or_else(|e| panic!("params deserialization for '{module_type}' failed: {e}"));
    let deserialized = DeserializedParams {
        params: cached.params,
        channel_count: cached.channel_count,
    };
    constructors
        .get(module_type)
        .unwrap_or_else(|| panic!("no constructor for '{module_type}'"))(
        &id.to_string(),
        SAMPLE_RATE,
        deserialized,
        TEST_BLOCK_SIZE,
        modular_core::types::ProcessingMode::Block,
    )
    .unwrap_or_else(|e| panic!("constructor for '{module_type}' failed: {e}"))
}

/// Per-sample cursor that hides block boundaries from tests. Each `tick()`
/// returns the slot index to read; when the cursor wraps past
/// `TEST_BLOCK_SIZE`, it triggers a fresh `start_block` + `ensure_processed`
/// on the module.
struct Stepper {
    slot: usize,
}

impl Stepper {
    fn new() -> Self {
        // Initialise out-of-range so the first tick triggers a block.
        Self {
            slot: TEST_BLOCK_SIZE,
        }
    }

    /// Advance one sample. Returns the slot index to read this frame's
    /// outputs from. Multiple reads in the same frame (e.g. L+R of a stereo
    /// output) share the returned slot.
    fn tick(&mut self, module: &dyn Sampleable) -> usize {
        if self.slot >= TEST_BLOCK_SIZE {
            module.start_block();
            module.ensure_processed();
            self.slot = 0;
        }
        let s = self.slot;
        self.slot += 1;
        s
    }
}

/// Advance N samples and collect the first channel of `output`.
fn collect_samples(module: &dyn Sampleable, n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(n);
    let mut s = Stepper::new();
    for _ in 0..n {
        let slot = s.tick(module);
        out.push(module.get_value_at(DEFAULT_PORT, 0, slot));
    }
    out
}

/// Collect N samples from a specific channel.
fn collect_channel(module: &dyn Sampleable, channel: usize, n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(n);
    let mut s = Stepper::new();
    for _ in 0..n {
        let slot = s.tick(module);
        out.push(module.get_value_at(DEFAULT_PORT, channel, slot));
    }
    out
}

/// Advance N samples and return the final sample read from channel 0 of
/// `DEFAULT_PORT`. Used by convergence tests that only care about the
/// post-settling value.
fn settle_and_read(module: &dyn Sampleable, n: usize) -> f32 {
    debug_assert!(n > 0, "settle_and_read needs at least one tick");
    let mut s = Stepper::new();
    let mut slot = 0;
    for _ in 0..n {
        slot = s.tick(module);
    }
    module.get_value_at(DEFAULT_PORT, 0, slot)
}

fn min_max(samples: &[f32]) -> (f32, f32) {
    let mn = samples.iter().cloned().fold(f32::INFINITY, f32::min);
    let mx = samples.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    (mn, mx)
}

/// Approximate equality within a tolerance.
fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

// ─── Sine oscillator ─────────────────────────────────────────────────────────

#[test]
fn sine_produces_bipolar_output() {
    let osc = make_module("$sine", "sine-1", json!({ "freq": 0.0 }));
    // 0 V/oct ≈ C4 (261.63 Hz)

    let samples = collect_samples(osc.as_ref(), 1000);
    let (mn, mx) = min_max(&samples);

    // Sine output should swing ±5 V
    assert!(mx > 4.5, "peak should be close to +5V, got {mx}");
    assert!(mn < -4.5, "trough should be close to -5V, got {mn}");
}

#[test]
fn sine_zero_frequency_is_dc() {
    let osc = make_module("$sine", "sine-1", json!({ "freq": -10.0 }));
    // Very low frequency → nearly DC over 100 samples

    let samples = collect_samples(osc.as_ref(), 100);
    let (mn, mx) = min_max(&samples);

    // At such a low frequency the output barely moves
    assert!(
        (mx - mn) < 1.0,
        "at very low freq, output should be near-DC; range was {}",
        mx - mn
    );
}

#[test]
fn sine_polyphonic() {
    let osc = make_module("$sine", "sine-1", json!({ "freq": [0.0, 1.0] }));

    let _ch0 = collect_channel(osc.as_ref(), 0, 500);
    // Reset for channel 1 read — we already stepped, so just read accumulated data
    // Actually the module already computed both channels per tick.
    // We need to re-check: collect_channel steps the module, so ch1 will be
    // from subsequent samples. That's fine — we just want to verify both channels
    // produce output.

    let osc2 = make_module("$sine", "sine-2", json!({ "freq": [0.0, 1.0] }));

    let mut ch0_samples = Vec::new();
    let mut ch1_samples = Vec::new();
    let mut s = Stepper::new();
    for _ in 0..500 {
        let slot = s.tick(osc2.as_ref());
        ch0_samples.push(osc2.get_value_at(DEFAULT_PORT, 0, slot));
        ch1_samples.push(osc2.get_value_at(DEFAULT_PORT, 1, slot));
    }

    let (mn0, mx0) = min_max(&ch0_samples);
    let (mn1, mx1) = min_max(&ch1_samples);

    assert!(mx0 > 4.0, "ch0 should oscillate, peak={mx0}");
    assert!(mn0 < -4.0, "ch0 should oscillate, trough={mn0}");
    assert!(mx1 > 4.0, "ch1 should oscillate, peak={mx1}");
    assert!(mn1 < -4.0, "ch1 should oscillate, trough={mn1}");

    // ch1 at higher V/oct should have different frequency (different waveform shape
    // over same number of samples)
    let sum0: f32 = ch0_samples.iter().map(|x| x.abs()).sum();
    let sum1: f32 = ch1_samples.iter().map(|x| x.abs()).sum();
    // They should differ because they're at different frequencies
    assert!(
        (sum0 - sum1).abs() > 0.1,
        "different pitches should produce different waveforms"
    );
}

// ─── Saw oscillator ──────────────────────────────────────────────────────────

#[test]
fn saw_produces_bipolar_output() {
    let osc = make_module("$saw", "saw-1", json!({ "freq": 0.0 }));

    let samples = collect_samples(osc.as_ref(), 1000);
    let (mn, mx) = min_max(&samples);

    assert!(mx > 4.0, "saw peak should be near +5V, got {mx}");
    assert!(mn < -4.0, "saw trough should be near -5V, got {mn}");
}

// ─── Pulse oscillator ────────────────────────────────────────────────────────

#[test]
fn pulse_produces_bipolar_output() {
    let osc = make_module("$pulse", "pulse-1", json!({ "freq": 0.0 }));

    let samples = collect_samples(osc.as_ref(), 1000);
    let (mn, mx) = min_max(&samples);

    assert!(mx > 4.0, "pulse peak should be near +5V, got {mx}");
    assert!(mn < -4.0, "pulse trough should be near -5V, got {mn}");
}

#[test]
fn pulse_width_affects_duty_cycle() {
    // Width 0 → near 50/50, width 5 → narrower positive
    let osc_narrow = make_module(
        "$pulse",
        "pulse-narrow",
        json!({ "freq": 0.0, "width": 4.0 }),
    );
    let samples_narrow = collect_samples(osc_narrow.as_ref(), 1000);

    let osc_wide = make_module("$pulse", "pulse-wide", json!({ "freq": 0.0, "width": 0.0 }));
    let samples_wide = collect_samples(osc_wide.as_ref(), 1000);

    // Count positive samples
    let pos_narrow = samples_narrow.iter().filter(|&&s| s > 0.0).count();
    let pos_wide = samples_wide.iter().filter(|&&s| s > 0.0).count();

    // Different widths should produce different ratios
    assert_ne!(
        pos_narrow, pos_wide,
        "different pulse widths should produce different duty cycles"
    );
}

// ─── Noise ───────────────────────────────────────────────────────────────────

#[test]
fn noise_produces_output() {
    let n = make_module("$noise", "noise-1", json!({ "color": "white" }));

    let samples = collect_samples(n.as_ref(), 1000);
    let (mn, mx) = min_max(&samples);

    assert!(mx > 0.5, "noise should have some positive values");
    assert!(mn < -0.5, "noise should have some negative values");

    // Check it's not DC — variance should be significant
    let mean: f32 = samples.iter().sum::<f32>() / samples.len() as f32;
    let variance: f32 =
        samples.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / samples.len() as f32;
    assert!(
        variance > 0.1,
        "white noise should have significant variance, got {variance}"
    );
}

// ─── ScaleAndShift ───────────────────────────────────────────────────────────

#[test]
fn scale_and_shift_applies() {
    let sas = make_module(
        "$scaleAndShift",
        "sas-1",
        json!({ "input": 1.0, "scale": 5.0, "shift": 2.0 }),
    );
    // Formula: output = input * (scale / 5.0) + shift
    // input=1.0, scale=5.0 (= 1x gain), shift=2.0 → output = 1.0 * 1.0 + 2.0 = 3.0

    // Step enough times for param smoothing to converge
    let sample = settle_and_read(sas.as_ref(), 500);
    assert!(approx_eq(sample, 3.0, 0.1), "expected ~3.0, got {sample}");
}

// ─── Constructors ────────────────────────────────────────────────────────────

/// Provide minimal required params for each module type so that deserialization
/// succeeds. Modules with all-optional params can use `{}`.
fn minimal_params(module_type: &str) -> serde_json::Value {
    match module_type {
        "$sine" | "$saw" | "$pulse" | "$supersaw" | "$ramp" => json!({ "freq": 0.0 }),
        "$pSine" | "$pSaw" | "$pPulse" => json!({ "phase": 0.0 }),
        "$macro" => json!({ "freq": 0.0, "engine": "virtualAnalog" }),
        "$lpf" | "$hpf" | "$jup6f" => json!({ "input": 0.0, "cutoff": 0.0 }),
        "$bpf" => json!({ "input": 0.0, "center": 0.0 }),
        "$xover" => json!({ "input": 0.0, "lowMidFreq": 0.0, "midHighFreq": 0.0 }),
        "$comp" => json!({ "input": 0.0, "threshold": 0.0 }),
        "$wrap" | "$clamp" => json!({ "input": 0.0, "min": -5.0, "max": 5.0 }),
        "$addHz" => json!({ "input": 0.0, "offset": 0.0 }),
        "$mulHz" => json!({ "input": 0.0, "factor": 1.0 }),
        "$curve" => json!({ "input": 0.0, "exp": 1.0 }),
        "$cycle" => json!({ "pattern": mini_payload("0") }),
        "$slew" | "$quantizer" | "$unison" | "$crush" | "$feedback" | "$pulsar" | "$rising"
        | "$falling" | "$stereoMix" | "$mixDown" => json!({ "input": 0.0 }),
        "$track" => json!({ "keyframes": [] }),
        "$math" => json!({ "expression": "1+1" }),
        "$spread" => json!({ "min": -1.0, "max": 1.0, "count": 3 }),
        "$signal" => json!({ "source": 0.0 }),
        "$scaleAndShift" => json!({ "input": 0.0 }),
        "$cheby" | "$fold" | "$segment" => json!({ "input": 0.0, "amount": 0.0 }),
        "$overdrive" => json!({ "input": 0.0, "drive": 0.0 }),
        "$buffer" => {
            json!({ "input": 0.0 })
        }
        "$bufRead" => {
            json!({ "buffer": { "type": "buffer_ref", "module": "test-module", "port": "buffer", "channels": 1 }, "frame": 0.0 })
        }
        "$delayRead" => {
            json!({ "buffer": { "type": "buffer_ref", "module": "test-module", "port": "buffer", "channels": 1 }, "time": 0.1 })
        }
        "$grains" => {
            json!({ "pitch": 0.0, "wav": { "type": "wav_ref", "path": "test", "channels": 1 }, "gate": 0.0 })
        }
        "$sampler" => {
            json!({ "wav": { "type": "wav_ref", "path": "test", "channels": 1 }, "gate": 0.0 })
        }
        "$wavetable" => {
            json!({ "wav": { "type": "wav_ref", "path": "test", "channels": 1 }, "pitch": 0.0 })
        }
        "$remap" => {
            json!({ "input": 0.0, "inMin": 0.0, "inMax": 5.0, "outMin": 0.0, "outMax": 5.0 })
        }
        "$mix" => json!({ "inputs": [] }),
        "$adsr" => json!({ "gate": 0.0 }),
        "$perc" => json!({ "trigger": 0.0 }),
        "$clockDivider" => json!({ "division": 2, "input": 0.0 }),
        "$sah" => json!({ "input": 0.0, "trigger": 0.0 }),
        "$tah" => json!({ "input": 0.0, "gate": 0.0 }),
        "$dattorro" => json!({ "input": 0.0 }),
        "$plate" => json!({ "input": 0.0 }),
        "$step" => json!({ "steps": [0.0], "next": 0.0 }),
        "$midiCC" => json!({ "cc": 1 }),
        "_clock" => json!({ "tempo": 120.0, "numerator": 4, "denominator": 4 }),
        _ => json!({}),
    }
}

#[test]
fn all_constructors_produce_valid_modules() {
    let constructors = get_constructors();
    let deserializers = get_params_deserializers();
    for (name, constructor) in &constructors {
        let deserializer = deserializers
            .get(name.as_str())
            .unwrap_or_else(|| panic!("no params deserializer for '{name}'"));
        let params = minimal_params(name);
        let cached = deserializer(params)
            .unwrap_or_else(|e| panic!("params deserialization for '{name}' failed: {e}"));
        let deserialized = DeserializedParams {
            params: cached.params,
            channel_count: cached.channel_count,
        };
        let module = constructor(
            &format!("test-{name}"),
            SAMPLE_RATE,
            deserialized,
            1,
            modular_core::types::ProcessingMode::Block,
        );
        assert!(
            module.is_ok(),
            "constructor for '{name}' should succeed, got: {:?}",
            module.err()
        );
        let module = module.unwrap();
        assert_eq!(module.get_module_type(), name);
    }
}

#[test]
fn all_constructors_can_tick() {
    let constructors = get_constructors();
    let deserializers = get_params_deserializers();
    for (name, constructor) in &constructors {
        let deserializer = deserializers
            .get(name.as_str())
            .unwrap_or_else(|| panic!("no params deserializer for '{name}'"));
        let params = minimal_params(name);
        let cached = deserializer(params)
            .unwrap_or_else(|e| panic!("params deserialization for '{name}' failed: {e}"));
        let deserialized = DeserializedParams {
            params: cached.params,
            channel_count: cached.channel_count,
        };
        let module = constructor(
            &format!("test-{name}"),
            SAMPLE_RATE,
            deserialized,
            1,
            modular_core::types::ProcessingMode::Block,
        )
        .unwrap();
        // Should not panic with minimal params
        module.start_block();
        module.ensure_processed();
        let _ = module.get_value_at(DEFAULT_PORT, 0, 0);
    }
}

// ─── Schema ──────────────────────────────────────────────────────────────────

#[test]
fn schema_names_match_constructors() {
    use modular_core::dsp::schema;
    let schemas = schema();
    let constructors = get_constructors();

    for s in &schemas {
        assert!(
            constructors.contains_key(&s.name),
            "schema '{}' has no matching constructor",
            s.name
        );
    }
}

#[test]
fn schemas_have_non_empty_documentation() {
    use modular_core::dsp::schema;
    for s in schema() {
        assert!(
            !s.documentation.is_empty(),
            "schema '{}' has empty documentation",
            s.name
        );
    }
}

// ─── Patch-level helpers ─────────────────────────────────────────────────────

/// Process one frame (single sample at `block_size=1`) of the entire patch.
/// Mirrors `AudioProcessor::process_frame`: reset cursors, then ensure every
/// module advances.
fn process_frame(patch: &Patch) {
    for module in patch.sampleables.values() {
        module.start_block();
    }
    for module in patch.sampleables.values() {
        module.ensure_processed();
    }
}

/// Helper to build a quick `PatchGraph` from a list of (id, module_type, params) tuples.
fn make_graph(modules: Vec<(&str, &str, serde_json::Value)>) -> PatchGraph {
    PatchGraph {
        modules: modules
            .into_iter()
            .map(|(id, module_type, params)| ModuleSpec {
                id: id.to_string(),
                module_type: module_type.to_string(),
                id_is_explicit: None,
                params,
            })
            .collect(),
        module_id_remaps: None,
        scopes: vec![],
        scope_xy: None,
    }
}

// ─── from_graph integration tests ────────────────────────────────────────────

#[test]
fn from_graph_creates_patch_with_modules() {
    let graph = make_graph(vec![
        ("osc1", "$sine", json!({ "freq": 0.0 })),
        ("osc2", "$saw", json!({ "freq": 1.0 })),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Both oscillators plus the hidden AudioIn
    assert!(patch.sampleables.contains_key("osc1"));
    assert!(patch.sampleables.contains_key("osc2"));
    assert!(patch.sampleables.contains_key("HIDDEN_AUDIO_IN"));
}

#[test]
fn from_graph_rejects_unknown_module_type() {
    let graph = make_graph(vec![("bad", "$nonexistent", json!({}))]);
    let result = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new());
    match result {
        Err(msg) => assert!(
            msg.contains("Unknown module type"),
            "error should mention unknown module type, got: {msg}"
        ),
        Ok(_) => panic!("expected error for unknown module type"),
    }
}

#[test]
fn from_graph_params_are_applied() {
    // Use $scaleAndShift with a constant input — its output should reflect the params.
    // Formula: output = input * (scale / 5.0) + shift
    // input=2.0, scale=5.0 (1× gain), shift=1.0 → output ≈ 3.0
    let graph = make_graph(vec![(
        "sas1",
        "$scaleAndShift",
        json!({ "input": 2.0, "scale": 5.0, "shift": 1.0 }),
    )]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Let param smoothing converge
    for _ in 0..500 {
        process_frame(&patch);
    }

    let module = patch.sampleables.get("sas1").unwrap();
    let sample = module.get_value_at(DEFAULT_PORT, 0, 0);
    assert!(
        approx_eq(sample, 3.0, 0.15),
        "expected ~3.0 after param smoothing, got {sample}"
    );
}

#[test]
fn from_graph_cable_routing_sine_to_signal() {
    // Sine oscillator → $signal module via cable.
    // The $signal module passes its `source` input straight through.
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 0.0 })),
        (
            "sig",
            "$signal",
            json!({
                "source": { "type": "cable", "module": "osc", "port": "output", "channel": 0 }
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Collect samples from the $signal module — it should reproduce the sine output
    let sig_module = patch.sampleables.get("sig").unwrap();
    let osc_module = patch.sampleables.get("osc").unwrap();

    let mut sig_samples = Vec::new();
    let mut osc_samples = Vec::new();
    for _ in 0..1000 {
        process_frame(&patch);
        sig_samples.push(sig_module.get_value_at(DEFAULT_PORT, 0, 0));
        osc_samples.push(osc_module.get_value_at(DEFAULT_PORT, 0, 0));
    }

    // The $signal output should match the oscillator's output exactly
    for (i, (s, o)) in sig_samples.iter().zip(osc_samples.iter()).enumerate() {
        assert!(
            approx_eq(*s, *o, 0.0001),
            "sample {i}: signal={s}, osc={o} — cable routing mismatch"
        );
    }

    // Verify the signal actually oscillates (not stuck at zero)
    let (mn, mx) = min_max(&sig_samples);
    assert!(mx > 4.0, "signal peak should be near +5V, got {mx}");
    assert!(mn < -4.0, "signal trough should be near -5V, got {mn}");
}

#[test]
fn from_graph_multi_module_osc_to_filter_to_mix() {
    // Build: sine oscillator → lowpass filter → mix → (read output)
    // The lowpass filter should attenuate high-frequency content.
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 3.0 })), // high freq ≈ 2093 Hz
        (
            "filt",
            "$lpf",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 },
                "cutoff": -2.0  // very low cutoff ≈ 65 Hz — should heavily attenuate
            }),
        ),
        (
            "mixer",
            "$mix",
            json!({
                "inputs": [
                    [{ "type": "cable", "module": "filt", "port": "output", "channel": 0 }]
                ]
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Also build a direct (unfiltered) patch for comparison
    let direct_graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 3.0 })),
        (
            "mixer",
            "$mix",
            json!({
                "inputs": [
                    [{ "type": "cable", "module": "osc", "port": "output", "channel": 0 }]
                ]
            }),
        ),
    ]);
    let direct_patch =
        Patch::from_graph(&direct_graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
            .expect("from_graph failed");

    // Let filter settle
    for _ in 0..500 {
        process_frame(&patch);
        process_frame(&direct_patch);
    }

    // Collect filtered and direct samples
    let mut filtered = Vec::new();
    let mut direct = Vec::new();
    let mix_filtered = patch.sampleables.get("mixer").unwrap();
    let mix_direct = direct_patch.sampleables.get("mixer").unwrap();

    for _ in 0..2000 {
        process_frame(&patch);
        process_frame(&direct_patch);
        filtered.push(mix_filtered.get_value_at(DEFAULT_PORT, 0, 0));
        direct.push(mix_direct.get_value_at(DEFAULT_PORT, 0, 0));
    }

    // Direct signal should have significant amplitude
    let (_, direct_mx) = min_max(&direct);
    assert!(
        direct_mx > 3.0,
        "direct sine should be loud, peak={direct_mx}"
    );

    // Filtered signal should have significantly lower amplitude (LPF attenuates)
    let rms_filtered = (filtered.iter().map(|s| s * s).sum::<f32>() / filtered.len() as f32).sqrt();
    let rms_direct = (direct.iter().map(|s| s * s).sum::<f32>() / direct.len() as f32).sqrt();

    assert!(
        rms_filtered < rms_direct * 0.5,
        "filtered RMS ({rms_filtered:.3}) should be much less than direct RMS ({rms_direct:.3})"
    );
}

#[test]
fn from_graph_process_frame_advances_all_modules() {
    // Two independent oscillators at different frequencies — both should produce output
    let graph = make_graph(vec![
        ("fast", "$sine", json!({ "freq": 3.0 })),
        ("slow", "$sine", json!({ "freq": -3.0 })),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    for _ in 0..500 {
        process_frame(&patch);
    }

    let fast = patch.sampleables.get("fast").unwrap();
    let slow = patch.sampleables.get("slow").unwrap();

    let mut fast_samples = Vec::new();
    let mut slow_samples = Vec::new();
    for _ in 0..2000 {
        process_frame(&patch);
        fast_samples.push(fast.get_value_at(DEFAULT_PORT, 0, 0));
        slow_samples.push(slow.get_value_at(DEFAULT_PORT, 0, 0));
    }

    let (fast_mn, fast_mx) = min_max(&fast_samples);
    let (slow_mn, slow_mx) = min_max(&slow_samples);

    assert!(fast_mx > 4.0, "fast osc should oscillate, peak={fast_mx}");
    assert!(
        fast_mn < -4.0,
        "fast osc should oscillate, trough={fast_mn}"
    );
    assert!(slow_mx > 4.0, "slow osc should oscillate, peak={slow_mx}");
    assert!(
        slow_mn < -4.0,
        "slow osc should oscillate, trough={slow_mn}"
    );
}

// ─── Step sequencer ──────────────────────────────────────────────────────────

#[test]
fn step_rejects_empty_steps() {
    let deserializers = get_params_deserializers();
    let deserializer = deserializers
        .get("$step")
        .expect("no deserializer for $step");
    let result = deserializer(json!({ "steps": [], "next": 0.0 }));
    match result {
        Ok(_) => panic!("empty steps should be rejected"),
        Err(err) => {
            let errors = err.into_errors();
            assert!(
                errors
                    .iter()
                    .any(|e| e.message.contains("at least one step")),
                "error should mention 'at least one step', got: {:?}",
                errors.iter().map(|e| &e.message).collect::<Vec<_>>()
            );
        }
    }
}

// ─── Curve ───────────────────────────────────────────────────────────────────

#[test]
fn curve_linear_passthrough() {
    // exp=1 should be linear: output ≈ input
    let m = make_module("$curve", "curve-1", json!({ "input": 3.0, "exp": 1.0 }));
    let sample = settle_and_read(m.as_ref(), 500);
    assert!(
        approx_eq(sample, 3.0, 0.1),
        "exp=1 should pass through, got {sample}"
    );
}

#[test]
fn curve_unity_at_5v() {
    // At 5V input, output should be 5V regardless of exponent
    let m = make_module("$curve", "curve-2", json!({ "input": 5.0, "exp": 3.0 }));
    let sample = settle_and_read(m.as_ref(), 500);
    assert!(
        approx_eq(sample, 5.0, 0.1),
        "5V should stay 5V, got {sample}"
    );
}

#[test]
fn curve_cubic_midpoint() {
    // exp=3, input=2.5: output = 5 * (2.5/5)^3 = 5 * 0.125 = 0.625
    let m = make_module("$curve", "curve-3", json!({ "input": 2.5, "exp": 3.0 }));
    let sample = settle_and_read(m.as_ref(), 500);
    assert!(
        approx_eq(sample, 0.625, 0.1),
        "expected ~0.625, got {sample}"
    );
}

#[test]
fn curve_preserves_sign() {
    // Negative input should produce negative output
    let m = make_module("$curve", "curve-4", json!({ "input": -2.5, "exp": 2.0 }));
    let sample = settle_and_read(m.as_ref(), 500);
    // sign(-2.5) * 5 * (2.5/5)^2 = -1 * 5 * 0.25 = -1.25
    assert!(
        approx_eq(sample, -1.25, 0.1),
        "expected ~-1.25, got {sample}"
    );
}

#[test]
fn curve_zero_input() {
    // Zero input should produce zero output
    let m = make_module("$curve", "curve-5", json!({ "input": 0.0, "exp": 3.0 }));
    let sample = settle_and_read(m.as_ref(), 500);
    assert!(
        approx_eq(sample, 0.0, 0.01),
        "0V input should produce 0V, got {sample}"
    );
}

#[test]
fn curve_exp_zero_step_function() {
    // exp=0: any nonzero input → ±5V
    let m = make_module("$curve", "curve-6", json!({ "input": 1.0, "exp": 0.0 }));
    let sample = settle_and_read(m.as_ref(), 500);
    assert!(
        approx_eq(sample, 5.0, 0.1),
        "exp=0 nonzero input should → 5V, got {sample}"
    );
}

// ─── Buffer + DelayRead pipeline ─────────────────────────────────────────────

#[test]
fn buffer_and_delay_read_pipeline() {
    // Feed a constant signal into $buffer, then read it back via $delayRead.
    // After the buffer fills past the delay time, every position holds the same
    // constant value, so the delayed read should converge to that value.
    //
    // Signal chain: $scaleAndShift(input=2, scale=5, shift=0) → $buffer → $delayRead
    // scale=5.0 means 1× gain, so output = 2.0 * 1.0 + 0.0 = 2.0
    let graph = make_graph(vec![
        (
            "sig",
            "$scaleAndShift",
            json!({ "input": 2.0, "scale": 5.0, "shift": 0.0 }),
        ),
        (
            "buf",
            "$buffer",
            json!({
                "input": { "type": "cable", "module": "sig", "port": "output", "channel": 0 },
                "length": 0.1
            }),
        ),
        (
            "delay",
            "$delayRead",
            json!({
                "buffer": { "type": "buffer_ref", "module": "buf", "port": "buffer", "channels": 1 },
                "time": 0.001
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // 0.001s delay at 48 kHz = 48 frames.
    // Process 500 frames so param smoothing converges and the buffer is well-filled.
    for _ in 0..500 {
        process_frame(&patch);
    }

    let delay_module = patch.sampleables.get("delay").unwrap();
    let sample = delay_module.get_value_at(DEFAULT_PORT, 0, 0);

    assert!(
        (sample - 2.0).abs() < 0.1,
        "delay read should output ~2.0 (constant input after filling), got {sample}"
    );
}

#[test]
fn buffer_feedback_cycle_propagates_through_delay_read() {
    // Verify that a cable cycle routed through a Buffer (delayRead's output
    // fed back into the buffer's input via mix) actually moves data around
    // the loop. If the cycle drops samples, delayRead never sees its own
    // output and the loop output stays pinned at the input value.
    //
    //   src ────┐
    //           ▼
    //          mix ──► buf ──► delayRead ──► feedback ──┐
    //           ▲                                       │
    //           └───────────────────────────────────────┘
    let graph = make_graph(vec![
        (
            "src",
            "$scaleAndShift",
            json!({ "input": 1.0, "scale": 5.0, "shift": 0.0 }),
        ),
        (
            "feedback",
            "$scaleAndShift",
            json!({
                "input": { "type": "cable", "module": "delayRead", "port": "output", "channel": 0 },
                "scale": 4.0,
                "shift": 0.0,
            }),
        ),
        (
            "mix",
            "$mix",
            json!({
                "inputs": [
                    { "type": "cable", "module": "src", "port": "output", "channel": 0 },
                    { "type": "cable", "module": "feedback", "port": "output", "channel": 0 },
                ],
            }),
        ),
        (
            "buf",
            "$buffer",
            json!({
                "input": { "type": "cable", "module": "mix", "port": "output", "channel": 0 },
                "length": 0.05,
            }),
        ),
        (
            "delayRead",
            "$delayRead",
            json!({
                "buffer": { "type": "buffer_ref", "module": "buf", "port": "buffer", "channels": 1 },
                "time": 0.01,
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    for _ in 0..20_000 {
        process_frame(&patch);
    }

    let dr_value = patch
        .sampleables
        .get("delayRead")
        .unwrap()
        .get_value_at(DEFAULT_PORT, 0, 0);

    // Steady state of 1 / (1 - 0.8) = 5. A dropped cycle pins delayRead at
    // the 1 V input.
    assert!(
        (dr_value - 5.0).abs() < 0.1,
        "feedback cycle did not converge to steady state; got dr={dr_value}"
    );
}

#[test]
fn delay_read_output_lags_behind_buffer_passthrough() {
    // Use a ramp signal (via $saw at a moderate frequency) as input to $buffer.
    // Compare the $buffer passthrough output to the $delayRead output.
    // Because $delayRead reads with a time offset, the two should differ on a
    // frame-by-frame basis when the input is changing.
    let graph = make_graph(vec![
        ("osc", "$saw", json!({ "freq": 0.0 })), // C4 ≈ 261 Hz — changes quickly
        (
            "buf",
            "$buffer",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 },
                "length": 0.1
            }),
        ),
        (
            "delay",
            "$delayRead",
            json!({
                "buffer": { "type": "buffer_ref", "module": "buf", "port": "buffer", "channels": 1 },
                "time": 0.005
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Let oscillator and buffer settle for 500 frames
    for _ in 0..500 {
        process_frame(&patch);
    }

    let buf_module = patch.sampleables.get("buf").unwrap();
    let delay_module = patch.sampleables.get("delay").unwrap();

    // Collect samples from both and count how many differ
    let mut differences = 0;
    let sample_count = 500;
    for _ in 0..sample_count {
        process_frame(&patch);
        let buf_sample = buf_module.get_value_at(DEFAULT_PORT, 0, 0);
        let delay_sample = delay_module.get_value_at(DEFAULT_PORT, 0, 0);
        if (buf_sample - delay_sample).abs() > 0.01 {
            differences += 1;
        }
    }

    // With a fast-changing signal and 0.005s delay (240 frames at 48kHz),
    // the delayed output should differ from the passthrough on most frames.
    assert!(
        differences > sample_count / 2,
        "delay read should lag behind buffer passthrough — only {differences}/{sample_count} samples differed"
    );
}

// ─── transfer_state_from wrapper output tests ────────────────────────────────

#[test]
fn transfer_state_from_preserves_wrapper_outputs_for_feedback_cycles() {
    // Bug: After transfer_state_from, the new module's wrapper outputs are
    // Default (zeros). In a feedback cycle, the module whose update() is
    // entered second reads the first module's wrapper outputs via
    // get_poly_sample(). If those are zeros instead of the previous frame's
    // values, a one-sample discontinuity is injected into the feedback loop.
    //
    // Setup: Two $scaleAndShift modules wired in a feedback cycle:
    //   A reads from B, B reads from A.
    // After running for several frames, we transfer state to new modules
    // and check that running one frame doesn't inject a zero discontinuity.
    //
    // Without the fix, whichever module is second in the cycle reads zeros
    // from the first module's wrapper on the transfer frame, producing an
    // output of `shift` instead of the correct feedback value.

    // Use $scaleAndShift: output = input * (scale / 5.0) + shift
    // A: input=B.output, scale=5.0 (gain=1.0), shift=1.0
    // B: input=A.output, scale=5.0 (gain=1.0), shift=0.0
    //
    // Steady state:
    //   A_out = B_out * 1.0 + 1.0
    //   B_out = A_out * 1.0 + 0.0 = A_out
    // So A_out = A_out + 1.0 diverges, but with the one-frame delay from the
    // cycle break it converges to a fixed-point quickly (the $scaleAndShift
    // just passes through with gain=1, so the cycle adds 1.0 per frame from
    // A's shift, growing until it clips).
    //
    // After ~100 frames, both outputs are large and non-zero. If the wrapper
    // outputs are not transferred, the cycle partner reads 0.0 on the first
    // frame, producing shift (1.0 or 0.0) instead of the previous large value.
    // We detect this by checking the output doesn't drop to near shift.

    let graph = make_graph(vec![
        (
            "a",
            "$scaleAndShift",
            json!({
                "input": { "type": "cable", "module": "b", "port": "output", "channel": 0 },
                "scale": 2.5,  // gain = 2.5/5.0 = 0.5 so it converges
                "shift": 1.0
            }),
        ),
        (
            "b",
            "$scaleAndShift",
            json!({
                "input": { "type": "cable", "module": "a", "port": "output", "channel": 0 },
                "scale": 2.5,  // gain = 0.5
                "shift": 0.0
            }),
        ),
    ]);

    let old_patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Run 200 frames to reach steady state
    // With gain=0.5 and shift=1.0:
    //   A_out = 0.5 * B_out + 1.0
    //   B_out = 0.5 * A_out(prev_frame)
    // Steady state: A=2.0, B=1.0
    for _ in 0..200 {
        process_frame(&old_patch);
    }

    let old_a_output = old_patch
        .sampleables
        .get("a")
        .unwrap()
        .get_value_at(DEFAULT_PORT, 0, 0);
    let old_b_output = old_patch
        .sampleables
        .get("b")
        .unwrap()
        .get_value_at(DEFAULT_PORT, 0, 0);

    // Verify we're at steady state with non-zero values
    assert!(
        old_a_output.abs() > 0.5,
        "module A should have substantial output at steady state, got {old_a_output}"
    );
    assert!(
        old_b_output.abs() > 0.1,
        "module B should have non-zero output at steady state, got {old_b_output}"
    );

    // Build a new patch with identical graph and transfer state
    let new_patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Transfer state from old modules to new modules
    for (id, new_module) in &new_patch.sampleables {
        if let Some(old_module) = old_patch.sampleables.get(id) {
            new_module.transfer_state_from(old_module.as_ref());
        }
    }

    // Reconnect (as apply_patch_update does)
    for module in new_patch.sampleables.values() {
        module.connect(&new_patch);
    }

    // Run ONE frame on the new patch — this is the transfer frame
    process_frame(&new_patch);

    let new_a_output = new_patch
        .sampleables
        .get("a")
        .unwrap()
        .get_value_at(DEFAULT_PORT, 0, 0);
    let new_b_output = new_patch
        .sampleables
        .get("b")
        .unwrap()
        .get_value_at(DEFAULT_PORT, 0, 0);

    // The outputs should be close to the old steady-state values.
    // Without the fix, one module reads zeros from the other's wrapper,
    // producing a value near its shift (1.0 for A, 0.0 for B) instead of
    // the correct feedback value.
    let a_delta = (new_a_output - old_a_output).abs();
    let b_delta = (new_b_output - old_b_output).abs();

    // Allow some tolerance for the one-frame evolution, but not a drop to
    // shift values. At steady state A≈2.0, B≈1.0. Without fix, one of them
    // drops to near its shift value (a jump of ~1.0).
    assert!(
        a_delta < 0.1,
        "module A output should be continuous across transfer.\n\
         Before: {old_a_output}, after: {new_a_output}, delta: {a_delta}\n\
         (large delta suggests wrapper outputs were not transferred)"
    );
    assert!(
        b_delta < 0.1,
        "module B output should be continuous across transfer.\n\
         Before: {old_b_output}, after: {new_b_output}, delta: {b_delta}\n\
         (large delta suggests wrapper outputs were not transferred)"
    );
}

// ─── $cycle($p.s) CV hold during rest after state transfer ───────────────────

#[test]
fn cycle_ps_cv_holds_during_rest_after_state_transfer() {
    // Regression (ported from the deleted `$iCycle` test, now lowered through
    // `$cycle($p.s(...))`): after a patch update (state transfer), a
    // scale-degree sequencer's CV output must HOLD the last active voltage
    // during a rest instead of dropping to 0 V. The `Seq` runtime keeps
    // `last_cv` per channel and only overwrites it while a voice is active;
    // `transfer_state_from` swaps the whole `SeqState`, so the held voltage
    // carries across the rebuild.
    //
    // Pattern `<0 ~>` in d#(min) alternates per cycle: degree 0 (D#4 = 0.25 V)
    // on cycle 0, a rest on cycle 1. At 48000 BPM / 4/4, one bar = 240 samples
    // at 48 kHz:
    //   Cycle 0 (samples 0..239):   degree 0, CV = 0.25 V (D#4)
    //   Cycle 1 (samples 240..479): rest, CV must HOLD at 0.25 V
    // Transfer state during cycle 1 (rest), run one frame, assert CV != 0.

    let graph = make_graph(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
        ),
        (
            "seq",
            "$cycle",
            json!({ "pattern": sp_payload(&["<0 ~>"], "d#(min)", vec![]) }),
        ),
    ]);

    let old_patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    // Advance through cycle 0 (degree 0) into the start of cycle 1 (rest).
    // One bar = 240 samples; process 260 to sit well inside cycle 1.
    for _ in 0..260 {
        process_frame(&old_patch);
    }

    // D#4 in V/Oct: (63 - 60) / 12 = 0.25 V.
    let expected_voltage = 0.25f32;

    let old_cv = old_patch
        .sampleables
        .get("seq")
        .unwrap()
        .get_value_at("cv", 0, 0);

    // Sanity: the OLD module holds the last active voltage during the rest.
    assert!(
        (old_cv - expected_voltage).abs() < 0.01,
        "old module CV should hold {expected_voltage} V during rest, got {old_cv}"
    );

    // Replicate apply_patch_update's reuse path: build a fresh patch, transfer
    // state, reconnect, on_patch_update. NO ClearPatch / transport reset.
    let new_patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    for (id, new_module) in &new_patch.sampleables {
        if let Some(old_module) = old_patch.sampleables.get(id) {
            new_module.transfer_state_from(old_module.as_ref());
        }
    }
    for module in new_patch.sampleables.values() {
        module.connect(&new_patch);
    }
    for module in new_patch.sampleables.values() {
        module.on_patch_update();
    }

    // Run ONE frame on the new patch — still in the rest period.
    process_frame(&new_patch);

    let new_cv = new_patch
        .sampleables
        .get("seq")
        .unwrap()
        .get_value_at("cv", 0, 0);

    // CV must still hold the previous active voltage, not collapse to 0.
    assert!(
        (new_cv - expected_voltage).abs() < 0.01,
        "after state transfer during rest, CV should hold {expected_voltage} V, got {new_cv}\n\
         (0.0 means last_cv was not preserved across state transfer)"
    );
}

// ─── Seq stale cached_hap survives transfer_state_from (highlight pin) ────────

/// Build the `$p.s(...)` chained payload wire shape that the TS `$p.s` helper
/// emits: `{ __kind: "SpPattern", sources, scale, ops, argument_spans }`.
/// Each source is a `ParsedPatternPayload` (`{ ast, source, all_spans }`).
fn sp_payload(sources: &[&str], scale: &str, ops: Vec<(&str, &str)>) -> Value {
    let srcs: Vec<Value> = sources.iter().map(|s| mini_payload(s)).collect();
    let ops_json: Vec<Value> = ops
        .into_iter()
        .map(|(op, mode)| json!({ "op": op, "mode": mode }))
        .collect();
    json!({
        "__kind": "SpPattern",
        "sources": srcs,
        "scale": scale,
        "ops": ops_json,
        "argument_spans": [],
    })
}

/// Total active highlight spans across every pattern source in a snapshot.
fn pod_total_spans(pod: &modular_core::dsp::seq::SeqHighlightState) -> usize {
    (0..modular_core::dsp::seq::highlight::MAX_SEQ_SOURCES)
        .map(|i| pod.spans_for(i).len())
        .sum()
}

/// Number of pattern sources carrying at least one active span in a snapshot.
fn pod_active_sources(pod: &modular_core::dsp::seq::SeqHighlightState) -> usize {
    (0..modular_core::dsp::seq::highlight::MAX_SEQ_SOURCES)
        .filter(|&i| !pod.spans_for(i).is_empty())
        .count()
}

/// Snapshot a module's live step-highlight spans and return the total active
/// span count across all pattern sources.
fn total_highlight_spans(module: &dyn modular_core::types::Sampleable) -> usize {
    let mut pod = modular_core::dsp::seq::SeqHighlightState::default();
    module.write_module_state(&mut pod);
    pod_total_spans(&pod)
}

#[test]
fn seq_highlight_survives_state_transfer_from_single_to_multi_source() {
    // Regression for the stale cached_hap highlight bug.
    //
    // On a live edit that turns `$cycle($p('[0 1 2 3 4 5 6] 5'))` into the
    // chained `$cycle($p.s('0 5').sub('2 4'))`, patchSimilarityRemap reuses the
    // Seq module. apply_patch_update rebuilds it with FRESH multi-source params
    // (is_multi_source=true, per_source=2, a freshly baked multi-source
    // cached_haps), but transfer_state_from std::mem::swaps the OLD SeqState
    // (voices[].cached_hap holding a hap_index computed against the OLD
    // single-source pattern) into the new module.
    //
    // The held voice still satisfies cached.contains(raw) off its scalar
    // raw_begin/raw_end window, so it is not released and no fresh onset fires
    // mid-step. The OLD pattern packed 7 sub-haps into the left half, so the
    // voice latched on the trailing `5` carries a hap_index (>= 2) that is OUT
    // OF RANGE in the new 2-hap multi-source storage — exactly the stale-index
    // symptom. But the held note's geometry (whole_begin=0.5, whole_end=1.0)
    // IS present in the new pattern: it's step 1 (`5 - 4 = 1`), now at a
    // different index. The fix self-heals the highlight read by re-resolving
    // the hap via geometry instead of trusting the stale index.
    //
    // Desired behaviour (this test asserts it): immediately after transfer +
    // on_patch_update — with no ClearPatch / transport restart and a voice
    // still held — get_state yields NON-EMPTY pattern.0 AND pattern.1 spans
    // matching the step the voice is actually playing. No re-latch needed.
    // The equal-geometry single-source case is kept as a negative control.

    // --- Patch A: single-source $cycle($p('[0 1 2 3 4 5 6] 5')). ---
    // The left group packs 7 sub-haps into 0.0..0.5; the trailing `5` occupies
    // 0.5..1.0. Latching on `5` gives a hap_index well past the 2 haps the new
    // multi-source pattern will hold, so the transferred index is out of range.
    let graph_a = make_graph(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
        ),
        (
            "seq",
            "$cycle",
            json!({ "pattern": mini_payload("[0 1 2 3 4 5 6] 5") }),
        ),
    ]);
    let old_patch = Patch::from_graph(&graph_a, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("A from_graph failed");

    // One bar = 240 samples. The trailing `5` covers 0.5..1.0 (samples
    // 120..240). Advance ~180 samples so the playhead sits at ~0.75, latching
    // the `5` hap (geometry whole_begin=0.5, whole_end=1.0).
    for _ in 0..180 {
        process_frame(&old_patch);
    }

    // Sanity: the OLD single-source module reports a non-empty highlight.
    let old_spans = total_highlight_spans(old_patch.sampleables.get("seq").unwrap().as_ref());
    assert!(
        old_spans > 0,
        "old single-source module should highlight the held `5` step; got {old_spans} spans"
    );

    // CV the held voice is producing. `5` is bare-number degree 5 → 5 V/oct.
    // We later confirm the read-only highlight resolver leaves it untouched.
    let old_cv = old_patch
        .sampleables
        .get("seq")
        .unwrap()
        .get_value_at("cv", 0, 0);

    // --- Patch B: chained $cycle($p.s('0 5').sub('2 4')) in c(maj). ---
    // 2 steps: step0 = 0-2 = -2, step1 = 5-4 = 1 — both non-rest degrees.
    // step1's geometry (0.5..1.0) matches the held voice's scalars; its
    // storage index (1) differs from the stale transferred index.
    let graph_b = make_graph(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
        ),
        (
            "seq",
            "$cycle",
            json!({ "pattern": sp_payload(&["0 5", "2 4"], "c(maj)", vec![("sub", "in")]) }),
        ),
    ]);
    let new_patch = Patch::from_graph(&graph_b, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("B from_graph failed");

    // Replicate apply_patch_update's reuse path: transfer state, reconnect,
    // on_patch_update. NO ClearPatch / transport reset.
    for (id, new_module) in &new_patch.sampleables {
        if let Some(old_module) = old_patch.sampleables.get(id) {
            new_module.transfer_state_from(old_module.as_ref());
        }
    }
    for module in new_patch.sampleables.values() {
        module.connect(&new_patch);
    }
    for module in new_patch.sampleables.values() {
        module.on_patch_update();
    }

    // Run ONE frame on B — still mid-step (playhead ~0.75 in step 1), the
    // swapped voice is held off its scalar whole_begin/whole_end, so no fresh
    // onset fires this frame.
    process_frame(&new_patch);

    let seq_b = new_patch.sampleables.get("seq").unwrap();
    let mut healed_pod = modular_core::dsp::seq::SeqHighlightState::default();
    seq_b.write_module_state(&mut healed_pod);
    let healed_spans = pod_total_spans(&healed_pod);

    let healed_active_sources = pod_active_sources(&healed_pod);
    eprintln!("--- seq highlight transfer (self-heal) ---");
    eprintln!("old (single-source) total spans = {old_spans}");
    eprintln!("after transfer, active sources   = {healed_active_sources}");
    eprintln!("after transfer, total spans      = {healed_spans}");

    // The fix: immediately after transfer (no restart), with a voice still
    // held off a stale/out-of-range hap_index, the highlight read self-heals
    // by geometry and surfaces the spans for the step actually playing.
    assert!(
        healed_spans > 0,
        "after state transfer with a held voice, the highlight should self-heal \
         (re-resolve the stale hap_index by geometry) and be NON-EMPTY. \
         Got total={healed_spans}, active sources={healed_active_sources}"
    );

    // Multi-source: both source 0 (`5`) and source 1 (`4`) — published under the
    // renderer keys `pattern.0`/`pattern.1` — contributed to the held step, so
    // both must carry spans.
    assert_eq!(
        healed_active_sources, 2,
        "expected two pattern sources to carry spans after the chained `$p.s` swap"
    );
    for source_idx in 0..2 {
        assert!(
            !healed_pod.spans_for(source_idx).is_empty(),
            "source {source_idx} (pattern.{source_idx}) should carry spans for the held step \
             after self-heal: {:?}",
            healed_pod.spans_for(source_idx)
        );
    }

    // Audio path untouched: get_state is read-only, so the held voice's CV is
    // still the value it was producing before (and after) the highlight read.
    // (`5` in c(maj) `$p.s('0 5').sub('2 4')` step1 = degree 1; we only check
    // the CV did not collapse to 0 — the resolver must not perturb voice
    // scalars.) The pre-transfer CV is the old single-source `5` = 5 V.
    let new_cv = seq_b.get_value_at("cv", 0, 0);
    assert!(
        new_cv.abs() > f32::EPSILON,
        "held voice CV should remain non-zero across the read-only highlight \
         resolve (audio path must be untouched); old_cv={old_cv}, new_cv={new_cv}"
    );

    // Negative control: an equal-geometry single-source re-run never had the
    // stale-index problem. Transferring into the SAME single-source pattern
    // keeps hap_index valid and highlight present — verifies the resolver's
    // trust-the-index fast path still works.
    let graph_c = make_graph(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
        ),
        (
            "seq",
            "$cycle",
            json!({ "pattern": mini_payload("[0 1 2 3 4 5 6] 5") }),
        ),
    ]);
    let ctrl_patch = Patch::from_graph(&graph_c, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("C from_graph failed");
    for (id, new_module) in &ctrl_patch.sampleables {
        if let Some(old_module) = old_patch.sampleables.get(id) {
            new_module.transfer_state_from(old_module.as_ref());
        }
    }
    for module in ctrl_patch.sampleables.values() {
        module.connect(&ctrl_patch);
    }
    for module in ctrl_patch.sampleables.values() {
        module.on_patch_update();
    }
    process_frame(&ctrl_patch);
    let ctrl_spans = total_highlight_spans(ctrl_patch.sampleables.get("seq").unwrap().as_ref());
    assert!(
        ctrl_spans > 0,
        "equal-geometry single-source re-run should keep highlight (index still \
         valid, fast path); got {ctrl_spans}"
    );
}

// ─── $track playhead boundaries ──────────────────────────────────────────────

/// Helper: run the module past initial param smoothing, then read channel 0.
fn track_value_at(playhead: f32, keyframes: serde_json::Value) -> f32 {
    let m = make_module(
        "$track",
        "track-1",
        json!({ "playhead": playhead, "keyframes": keyframes }),
    );
    settle_and_read(m.as_ref(), 500)
}

#[test]
fn track_last_keyframe_at_time_1_reachable_with_playhead_1() {
    // Regression: previously `fract(1.0) == 0.0` mapped playhead=1 to t=0,
    // clamping to the first keyframe and making the time=1 keyframe unreachable.
    let v = track_value_at(1.0, json!([[1.0, 0.0], [5.0, 1.0]]));
    assert!(approx_eq(v, 5.0, 0.01), "expected 5.0, got {v}");
}

#[test]
fn track_first_keyframe_at_time_0_with_playhead_0() {
    let v = track_value_at(0.0, json!([[1.0, 0.0], [5.0, 1.0]]));
    assert!(approx_eq(v, 1.0, 0.01), "expected 1.0, got {v}");
}

#[test]
fn track_midpoint_interpolates_linearly() {
    let v = track_value_at(0.5, json!([[1.0, 0.0], [5.0, 0.5], [3.0, 1.0]]));
    assert!(approx_eq(v, 5.0, 0.01), "expected 5.0, got {v}");
}

#[test]
fn track_clamps_playhead_above_one() {
    let v = track_value_at(1.5, json!([[1.0, 0.0], [5.0, 1.0]]));
    assert!(approx_eq(v, 5.0, 0.01), "expected 5.0 (clamped), got {v}");
}

#[test]
fn track_clamps_playhead_below_zero() {
    let v = track_value_at(-0.1, json!([[1.0, 0.0], [5.0, 1.0]]));
    assert!(approx_eq(v, 1.0, 0.01), "expected 1.0 (clamped), got {v}");
}

// ─── $cycle ribbon loop window ────────────────────────────────────────────────

/// Build a `$cycle` patch clocked at 48000 BPM 4/4 (240 samples per cycle),
/// optionally with a `ribbon: [offset, length]` window.
fn make_cycle_patch(pattern: Value, ribbon: Option<[u64; 2]>) -> Patch {
    make_cycle_patch_ribbon(
        pattern,
        ribbon.map(|[offset, length]| json!([offset, length])),
    )
}

/// Like [`make_cycle_patch`] but with a fractional `[offset, length]` ribbon.
fn make_cycle_patch_frac(pattern: Value, ribbon: [f64; 2]) -> Patch {
    let [offset, length] = ribbon;
    make_cycle_patch_ribbon(pattern, Some(json!([offset, length])))
}

fn make_cycle_patch_ribbon(pattern: Value, ribbon: Option<Value>) -> Patch {
    let mut params = serde_json::Map::new();
    params.insert("pattern".to_string(), pattern);
    if let Some(ribbon) = ribbon {
        params.insert("ribbon".to_string(), ribbon);
    }
    let graph = make_graph(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
        ),
        ("seq", "$cycle", Value::Object(params)),
    ]);
    Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed")
}

/// Sample `port` at the midpoint of cycles `0..num_cycles` (240 samples/cycle).
fn cycle_midpoint_values(patch: &Patch, port: &str, num_cycles: usize) -> Vec<f32> {
    const SPC: usize = 240;
    let targets: Vec<usize> = (0..num_cycles).map(|c| c * SPC + SPC / 2).collect();
    let total = *targets.last().unwrap();
    let mut out = Vec::with_capacity(num_cycles);
    let mut ti = 0;
    for s in 1..=total {
        process_frame(patch);
        if ti < targets.len() && targets[ti] == s {
            out.push(
                patch
                    .sampleables
                    .get("seq")
                    .unwrap()
                    .get_value_at(port, 0, 0),
            );
            ti += 1;
        }
    }
    out
}

/// Per-sample trace of `port` over `total_samples` frames.
fn cycle_port_trace(patch: &Patch, port: &str, total_samples: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(total_samples);
    for _ in 0..total_samples {
        process_frame(patch);
        out.push(
            patch
                .sampleables
                .get("seq")
                .unwrap()
                .get_value_at(port, 0, 0),
        );
    }
    out
}

#[test]
fn seq_ribbon_default_window_plays_pattern_through() {
    // Default ribbon [0, 1024]: the slowcat `<c4 e4 g4 b4>` advances every
    // cycle for the first 4 cycles (far inside the window), so all four
    // differ — identical to a direct pattern query, no early looping.
    let patch = make_cycle_patch(mini_payload("<c4 e4 g4 b4>"), None);
    let cv = cycle_midpoint_values(&patch, "cv", 4);
    for i in 0..4 {
        for j in (i + 1)..4 {
            assert!(
                (cv[i] - cv[j]).abs() > 0.05,
                "default window should play 4 distinct cycles; cv[{i}]={} cv[{j}]={}",
                cv[i],
                cv[j]
            );
        }
    }
}

#[test]
fn seq_ribbon_loops_window() {
    // ribbon [0, 2] bakes only cycles 0,1 of `<c4 e4 g4 b4>`, then loops them:
    // clock cycles 2,3 replay baked cycles 0,1 instead of advancing to g4,b4.
    let looped = cycle_midpoint_values(
        &make_cycle_patch(mini_payload("<c4 e4 g4 b4>"), Some([0, 2])),
        "cv",
        4,
    );
    assert!(
        (looped[0] - looped[1]).abs() > 0.05,
        "the window's two cycles differ: {} vs {}",
        looped[0],
        looped[1]
    );
    assert!(
        approx_eq(looped[2], looped[0], 0.01),
        "cycle 2 loops back to cycle 0: {} vs {}",
        looped[2],
        looped[0]
    );
    assert!(
        approx_eq(looped[3], looped[1], 0.01),
        "cycle 3 loops back to cycle 1: {} vs {}",
        looped[3],
        looped[1]
    );

    // Without the ribbon the same clock cycle 2 advances to a different note,
    // proving the ribbon — not the pattern — drives the loop.
    let unlooped = cycle_midpoint_values(
        &make_cycle_patch(mini_payload("<c4 e4 g4 b4>"), None),
        "cv",
        4,
    );
    assert!(
        (looped[2] - unlooped[2]).abs() > 0.05,
        "ribbon changes cycle-2 output: looped {} vs default {}",
        looped[2],
        unlooped[2]
    );
}

#[test]
fn seq_ribbon_offset_window_plays_and_loops() {
    // ribbon [2, 2] bakes cycles 2,3 and loops them: clock cycle 0 plays the
    // pattern's cycle 2, clock cycle 1 plays cycle 3, then it repeats.
    let reference = cycle_midpoint_values(
        &make_cycle_patch(mini_payload("<c4 e4 g4 b4>"), None),
        "cv",
        4,
    );
    let offset = cycle_midpoint_values(
        &make_cycle_patch(mini_payload("<c4 e4 g4 b4>"), Some([2, 2])),
        "cv",
        4,
    );
    assert!(
        approx_eq(offset[0], reference[2], 0.01),
        "offset cycle 0 == pattern cycle 2: {} vs {}",
        offset[0],
        reference[2]
    );
    assert!(
        approx_eq(offset[1], reference[3], 0.01),
        "offset cycle 1 == pattern cycle 3: {} vs {}",
        offset[1],
        reference[3]
    );
    assert!(
        approx_eq(offset[2], offset[0], 0.01),
        "offset window loops: cycle 2 == cycle 0"
    );
    assert!(
        approx_eq(offset[3], offset[1], 0.01),
        "offset window loops: cycle 3 == cycle 1"
    );
}

#[test]
fn seq_ribbon_wrap_note_plays_full_length_then_releases_once() {
    // `c4/3` slows c4 over 3 cycles: onset at cycle 0, whole span [0, 3].
    // ribbon [0, 2] bakes cycles 0,1 only, so the note straddles the loop seam
    // at the start of clock cycle 2. It must keep sounding through cycle 2
    // (its full 3-cycle length, past the wrap), then release exactly once at
    // cycle 3 — never cut early, never stuck.
    let gate = cycle_port_trace(
        &make_cycle_patch(mini_payload("c4/3"), Some([0, 2])),
        "gate",
        4 * 240,
    );
    for (cyc, s) in [(0usize, 120usize), (1, 360), (2, 600)] {
        assert!(
            gate[s] > 2.5,
            "gate should be HIGH mid-cycle {cyc} (sample {s}) — note plays past the wrap, got {}",
            gate[s]
        );
    }
    assert!(
        gate[840] < 2.5,
        "gate should be LOW mid-cycle 3 — the note released, got {}",
        gate[840]
    );

    // The looped slot re-presents the same onset hap at the seam (clock cycle
    // 2 maps back to baked cycle 0), but a note longer than the window must
    // NOT re-trigger while it is still sounding: exactly one onset over the
    // three cycles it plays.
    let trig = cycle_port_trace(
        &make_cycle_patch(mini_payload("c4/3"), Some([0, 2])),
        "trig",
        3 * 240,
    );
    let mut onsets = 0;
    let mut prev_high = false;
    for &v in &trig {
        let high = v > 2.5;
        if high && !prev_high {
            onsets += 1;
        }
        prev_high = high;
    }
    assert_eq!(
        onsets, 1,
        "a note longer than the window fires exactly one onset while sounding (no seam re-trigger)"
    );
}

#[test]
fn seq_ribbon_rejects_invalid_bounds() {
    let attempt = |ribbon: Value| {
        let graph = make_graph(vec![
            (
                "ROOT_CLOCK",
                "_clock",
                json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
            ),
            (
                "seq",
                "$cycle",
                json!({ "pattern": mini_payload("c4 e4"), "ribbon": ribbon }),
            ),
        ]);
        Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
    };

    match attempt(json!([0, 0])) {
        Err(msg) => assert!(
            msg.contains("ribbon loop length must be greater than 0"),
            "got: {msg}"
        ),
        Ok(_) => panic!("expected error for zero ribbon length"),
    }
    match attempt(json!([0, 9000])) {
        Err(msg) => assert!(msg.contains("8192 cycles or fewer"), "got: {msg}"),
        Ok(_) => panic!("expected error for over-cap ribbon length"),
    }
    match attempt(json!([2_000_000, 4])) {
        Err(msg) => assert!(msg.contains("ribbon offset must be"), "got: {msg}"),
        Ok(_) => panic!("expected error for over-cap ribbon offset"),
    }
    // A negative offset is now rejected by the bounds hook (not structurally,
    // since `f64` accepts it).
    match attempt(json!([-1, 4])) {
        Err(msg) => assert!(
            msg.contains("ribbon offset must be 0 or greater"),
            "got: {msg}"
        ),
        Ok(_) => panic!("expected error for negative ribbon offset"),
    }
    // Fractional values are now VALID — a fractional window is the whole point.
    assert!(
        attempt(json!([0.5, 4])).is_ok(),
        "fractional ribbon must be accepted"
    );
}

/// Count low→high transitions in a port trace.
fn rising_edges(trace: &[f32]) -> usize {
    let mut n = 0;
    let mut prev = false;
    for &v in trace {
        let high = v > 2.5;
        if high && !prev {
            n += 1;
        }
        prev = high;
    }
    n
}

/// For a `$cycle` patch: gate state at each cycle midpoint (`true` = sounding)
/// and the trig onset count over `cycles`. (A trig onset leads its gate rising
/// edge by the min-gate hold, so terminate the trace in a silent stretch to
/// keep the onset count free of boundary-straddle artifacts.)
fn cycle_gate_and_onsets(pattern: &str, ribbon: [u64; 2], cycles: usize) -> (Vec<bool>, usize) {
    const SPC: usize = 240;
    let gate = cycle_port_trace(
        &make_cycle_patch(mini_payload(pattern), Some(ribbon)),
        "gate",
        cycles * SPC,
    );
    let trig = cycle_port_trace(
        &make_cycle_patch(mini_payload(pattern), Some(ribbon)),
        "trig",
        cycles * SPC,
    );
    let mids = (0..cycles).map(|c| gate[c * SPC + SPC / 2] > 2.5).collect();
    (mids, rising_edges(&trig))
}

/// A note LONGER than the ribbon window plays its full length, then goes
/// silent until the window's loop point realigns with the onset — a
/// deterministic gap, never a stuck note, a double-trigger, or an early cut.
///
/// The onset hap lives only at baked cycle `offset` (slot 0), which recurs
/// every `length` clock cycles. `c4/3` (3-cycle note) in a 2-cycle window
/// releases at clock cycle 3, but slot 0 next comes around at clock cycle 4,
/// so it re-onsets there: 3 cycles sounding + 1 cycle gap, period 4.
#[test]
fn seq_ribbon_note_longer_than_window_plays_full_then_gaps() {
    // Trace 11 cycles — ends in the third gap, so the next group's onset is
    // outside the window and the onset count is exactly one-per-group.
    let (mids, onsets) = cycle_gate_and_onsets("c4/3", [0, 2], 11);
    let expected = [
        true, true, true, false, // sound 0,1,2; gap 3
        true, true, true, false, // sound 4,5,6; gap 7
        true, true, true, // sound 8,9,10  (gap 11 not traced)
    ];
    assert_eq!(
        mids, expected,
        "3-cycle note in a 2-cycle window: 3 sounding + 1 gap, period 4"
    );
    // Three sounding groups, one onset each — no seam re-trigger while a note
    // is still sounding, no double-fire.
    assert_eq!(onsets, 3, "exactly one onset per sounding group");
}

/// A note whose length divides the ribbon window loops with NO gap: it
/// re-articulates exactly at the loop point (a fresh trig each lap), and the
/// gate is continuous across cycle midpoints. `c4/2` (2-cycle note, window 2)
/// and `c4/4` (4-cycle note, window 2 — 4 % 2 == 0) both loop seamlessly.
#[test]
fn seq_ribbon_note_dividing_window_loops_seamlessly() {
    let (mids2, onsets2) = cycle_gate_and_onsets("c4/2", [0, 2], 8);
    assert!(
        mids2.iter().all(|&g| g),
        "2-cycle note in a 2-cycle window sounds at every cycle midpoint (no gap): {mids2:?}"
    );
    // Re-articulates each lap (a fresh trig per loop) rather than holding one
    // gate forever — looping a held note re-triggers it.
    assert!(
        onsets2 >= 3,
        "re-triggers once per 2-cycle loop over 8 cycles: {onsets2}"
    );

    let (mids4, _) = cycle_gate_and_onsets("c4/4", [0, 2], 8);
    assert!(
        mids4.iter().all(|&g| g),
        "4-cycle note (4 % 2 == 0) also loops with no gap: {mids4:?}"
    );
}

/// True if any sample in `trace` is within `eps` of `target`.
fn trace_contains(trace: &[f32], target: f32, eps: f32) -> bool {
    trace.iter().any(|&v| (v - target).abs() < eps)
}

/// A fractional ribbon LENGTH defines a loop window whose seam falls mid-cycle.
/// `ribbon:[0, 1.5]` bakes the slice of cycles 0 and 1 the window touches and
/// loops with period 1.5 cycles (360 samples): the slowcat only ever plays its
/// cycle-0 (`c4`) and cycle-1 (`e4`) notes — never cycle 2 (`g4`) or cycle 3
/// (`b4`) — and the cv trace repeats every 1.5 cycles in steady state.
#[test]
fn seq_ribbon_fractional_length_loops() {
    const SPC: usize = 240;
    // Reference voltages for the slowcat's first four cycles.
    let notes = cycle_midpoint_values(
        &make_cycle_patch(mini_payload("<c4 e4 g4 b4>"), None),
        "cv",
        4,
    );
    let (c4, e4, g4, b4) = (notes[0], notes[1], notes[2], notes[3]);

    let trace = cycle_port_trace(
        &make_cycle_patch_frac(mini_payload("<c4 e4 g4 b4>"), [0.0, 1.5]),
        "cv",
        9 * SPC,
    );

    // The window plays both of the cycles it touches...
    assert!(
        trace_contains(&trace, c4, 0.01),
        "window plays cycle 0 (c4)"
    );
    assert!(
        trace_contains(&trace, e4, 0.01),
        "window plays cycle 1 (e4)"
    );
    // ...and never the cycles beyond it.
    assert!(
        !trace_contains(&trace, g4, 0.01),
        "fractional window [0,1.5) never reaches cycle 2 (g4)"
    );
    assert!(
        !trace_contains(&trace, b4, 0.01),
        "fractional window [0,1.5) never reaches cycle 3 (b4)"
    );

    // Loops with period 1.5 cycles (360 samples), checked in steady state
    // (after the first 4 cycles of warm-up).
    let period = 360; // 1.5 * 240
    for s in (4 * SPC)..(6 * SPC) {
        assert!(
            approx_eq(trace[s], trace[s + period], 0.01),
            "cv repeats every 1.5 cycles: sample {s}={} vs {}={}",
            trace[s],
            s + period,
            trace[s + period]
        );
    }
}

/// A fractional ribbon OFFSET starts the loop window partway into the pattern.
/// `ribbon:[0.5, 2]` covers pattern positions [0.5, 2.5): cycle 0's second
/// half, all of cycle 1, and cycle 2's first half — so the slowcat plays `c4`,
/// `e4` AND `g4` (reaching into cycle 2, which an integer `[0,2]` window never
/// would), but never cycle 3's `b4`. It loops with period 2 cycles.
#[test]
fn seq_ribbon_fractional_offset_window() {
    const SPC: usize = 240;
    let notes = cycle_midpoint_values(
        &make_cycle_patch(mini_payload("<c4 e4 g4 b4>"), None),
        "cv",
        4,
    );
    let (c4, e4, g4, b4) = (notes[0], notes[1], notes[2], notes[3]);

    let trace = cycle_port_trace(
        &make_cycle_patch_frac(mini_payload("<c4 e4 g4 b4>"), [0.5, 2.0]),
        "cv",
        8 * SPC,
    );

    assert!(
        trace_contains(&trace, c4, 0.01),
        "window includes cycle 0 (c4)"
    );
    assert!(
        trace_contains(&trace, e4, 0.01),
        "window includes cycle 1 (e4)"
    );
    assert!(
        trace_contains(&trace, g4, 0.01),
        "fractional offset 0.5 reaches into cycle 2 (g4)"
    );
    assert!(
        !trace_contains(&trace, b4, 0.01),
        "window [0.5,2.5) never reaches cycle 3 (b4)"
    );

    // Loops with period 2 cycles (480 samples) in steady state.
    for s in (4 * SPC)..(6 * SPC) {
        assert!(
            approx_eq(trace[s], trace[s + 2 * SPC], 0.01),
            "cv repeats every 2 cycles: sample {s} vs {}",
            s + 2 * SPC
        );
    }
}

// ─── $cycle voice leading, edge-triggered onsets, new-pattern voice stealing ──

/// Build a `$cycle` patch with optional explicit `channels` and `playhead`.
/// A `Some(playhead)` overrides the clock connection with a constant position
/// (`Signal::Volts`, read with no smoothing), so a test can place the playhead
/// arbitrarily — including moving it backward across rebuild+transfer steps.
fn cycle_patch(pattern: Value, channels: Option<u64>, playhead: Option<f64>) -> Patch {
    let mut params = serde_json::Map::new();
    params.insert("pattern".to_string(), pattern);
    if let Some(ch) = channels {
        params.insert("channels".to_string(), json!(ch));
    }
    if let Some(ph) = playhead {
        params.insert("playhead".to_string(), json!(ph));
    }
    let graph = make_graph(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
        ),
        ("seq", "$cycle", Value::Object(params)),
    ]);
    Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed")
}

/// Replicate `apply_patch_update`'s reuse path: transfer state, reconnect,
/// `on_patch_update`. No ClearPatch / transport reset — so `prev_logical` and the
/// held voices carry from `old` into `new`, exactly as a live edit does.
fn transfer_into(new_patch: &Patch, old_patch: &Patch) {
    for (id, m) in &new_patch.sampleables {
        if let Some(old) = old_patch.sampleables.get(id) {
            m.transfer_state_from(old.as_ref());
        }
    }
    for m in new_patch.sampleables.values() {
        m.connect(new_patch);
    }
    for m in new_patch.sampleables.values() {
        m.on_patch_update();
    }
}

/// Read channel `ch` of `port` on the `seq` module at the current frame.
fn seq_val(patch: &Patch, port: &str, ch: usize) -> f32 {
    patch
        .sampleables
        .get("seq")
        .unwrap()
        .get_value_at(port, ch, 0)
}

#[test]
fn cycle_swap_does_not_fire_in_progress_note_until_its_onset() {
    // A new pattern's note whose window already contains the playhead at swap
    // time must NOT fire mid-window — even with a free voice available
    // (channels=2). It fires only when the playhead next crosses its onset.
    // (Old level-detection would have triggered it immediately on the free voice.)
    let a = cycle_patch(mini_payload("c4"), Some(2), None);
    for _ in 0..120 {
        process_frame(&a); // advance to mid-cycle 0 (playhead ~0.5)
    }
    let b = cycle_patch(mini_payload("e4"), Some(2), None);
    transfer_into(&b, &a);

    // Rest of cycle 0 after the swap: e4's window [0,1] already holds the
    // playhead, so no onset fires.
    let mut prev_hi = [false; 2];
    let mut onsets_cycle0 = 0;
    for _ in 0..100 {
        process_frame(&b);
        for ch in 0..2 {
            let hi = seq_val(&b, "trig", ch) > 2.5;
            if hi && !prev_hi[ch] {
                onsets_cycle0 += 1;
            }
            prev_hi[ch] = hi;
        }
    }
    assert_eq!(
        onsets_cycle0, 0,
        "no onset should fire mid-window on a pattern swap (a free voice is present)"
    );

    // Cross into cycle 1: e4's real onset is crossed → it fires.
    let mut onsets_cycle1 = 0;
    for _ in 0..200 {
        process_frame(&b);
        for ch in 0..2 {
            let hi = seq_val(&b, "trig", ch) > 2.5;
            if hi && !prev_hi[ch] {
                onsets_cycle1 += 1;
            }
            prev_hi[ch] = hi;
        }
    }
    assert!(
        onsets_cycle1 >= 1,
        "e4 must fire when the playhead crosses its onset in the next cycle"
    );
}

#[test]
fn cycle_onset_fires_on_backward_entry() {
    // The playhead is an arbitrary signal; a hap fires on ENTERING its window
    // from EITHER direction. `g4 c5`: g4 window [0,0.5], c5 window [0.5,1].
    // Frame 1 at playhead 0.6 plays c5. Frame 2 moves the playhead BACKWARD to
    // 0.4 — entering g4's window from the right — which must fire g4.
    let a = cycle_patch(mini_payload("g4 c5"), None, Some(0.6));
    process_frame(&a);
    let cv1 = seq_val(&a, "cv", 0);
    assert!(
        (cv1 - 1.0).abs() < 0.02,
        "frame 1 at playhead 0.6 should play c5 (1 V), got {cv1}"
    );

    let b = cycle_patch(mini_payload("g4 c5"), None, Some(0.4));
    transfer_into(&b, &a);
    process_frame(&b);
    let trig = seq_val(&b, "trig", 0);
    let cv2 = seq_val(&b, "cv", 0);
    assert!(
        trig > 2.5,
        "entering g4's window from the right (backward) must fire a trig, got {trig}"
    );
    assert!(
        (cv2 - 0.5833).abs() < 0.02,
        "cv should be g4 (~0.583 V) after backward entry, got {cv2}"
    );
}

#[test]
fn cycle_new_pattern_steals_orphaned_voice() {
    // An old long note (`c3/4`, 4-cycle) holds the only voice (channels=1). After
    // swapping to `e4 g4`, the new onsets — which the playhead crosses in cycle 1
    // — must STEAL the lingering c3 voice rather than be dropped. Without stealing
    // the old c3 (-1 V) would keep sounding for four cycles.
    let a = cycle_patch(mini_payload("c3/4"), Some(1), None);
    for _ in 0..120 {
        process_frame(&a);
    }
    let cv0 = seq_val(&a, "cv", 0);
    assert!(
        (cv0 - (-1.0)).abs() < 0.02,
        "old c3/4 should sound at -1 V before the swap, got {cv0}"
    );

    let b = cycle_patch(mini_payload("e4 g4"), Some(1), None);
    transfer_into(&b, &a);

    // Advance through cycle 1; the new pattern's onsets steal the c3 voice.
    let mut saw_new_trig = false;
    let mut max_cv = f32::MIN;
    for _ in 0..240 {
        process_frame(&b);
        if seq_val(&b, "trig", 0) > 2.5 {
            saw_new_trig = true;
        }
        max_cv = max_cv.max(seq_val(&b, "cv", 0));
    }
    assert!(
        saw_new_trig,
        "the new pattern's onset must fire by stealing the orphaned c3 voice"
    );
    assert!(
        max_cv > 0.2,
        "cv must reach the new pattern's positive notes (e4/g4), got max {max_cv}"
    );
    assert!(
        seq_val(&b, "cv", 0) > -0.5,
        "the old c3 (-1 V) must no longer be sounding after the steal"
    );
}

#[test]
fn cycle_allocates_nearest_value_voice() {
    // Voice leading: `c4 c5, c5 c4` layers two mono lines so each half-cycle
    // boundary re-onsets one c4 and one c5. Nearest-value allocation keeps each
    // physical voice on its pitch across the boundary (the voice holding c4 takes
    // the new c4, etc.), so one channel stays ~0 V and the other ~1 V throughout.
    // Round-robin would swap the lanes every half cycle.
    let patch = cycle_patch(mini_payload("c4 c5, c5 c4"), None, None);

    let mut ch0 = [0f32; 2];
    let mut ch1 = [0f32; 2];
    for s in 1..=180 {
        process_frame(&patch);
        if s == 60 {
            // first half-slot [0,0.5]
            ch0[0] = seq_val(&patch, "cv", 0);
            ch1[0] = seq_val(&patch, "cv", 1);
        }
        if s == 180 {
            // second half-slot [0.5,1]
            ch0[1] = seq_val(&patch, "cv", 0);
            ch1[1] = seq_val(&patch, "cv", 1);
        }
    }
    let near0 = |v: f32| v.abs() < 0.05;
    let near1 = |v: f32| (v - 1.0).abs() < 0.05;
    let lane_a = near0(ch0[0]) && near0(ch0[1]) && near1(ch1[0]) && near1(ch1[1]);
    let lane_b = near1(ch0[0]) && near1(ch0[1]) && near0(ch1[0]) && near0(ch1[1]);
    assert!(
        lane_a || lane_b,
        "each voice must keep its pitch across the boundary (no lane swap): \
         ch0=[{},{}] ch1=[{},{}]",
        ch0[0],
        ch0[1],
        ch1[0],
        ch1[1]
    );
}

#[test]
fn cycle_simultaneous_identical_notes_keep_separate_voices() {
    // `c4,c4` layers two whole-cycle c4 haps. Both must get their own voice (two
    // active gates) — the joint nearest-value assignment must not collapse
    // identical-value simultaneous onsets onto one voice. Read after the
    // ~16-sample gate retrigger gap.
    let patch = cycle_patch(mini_payload("c4,c4"), None, None);
    for _ in 0..40 {
        process_frame(&patch);
    }
    let g0 = seq_val(&patch, "gate", 0);
    let g1 = seq_val(&patch, "gate", 1);
    assert!(
        g0 > 2.5 && g1 > 2.5,
        "both voices should be gated high for `c4,c4`: g0={g0} g1={g1}"
    );
}

#[test]
fn cycle_swap_to_same_note_does_not_retrigger() {
    // Editing a pattern while a note sounds must not re-trigger that note if the
    // new pattern keeps it (same window+value): the gate stays continuously high
    // and no fresh trig fires (a continuation, not a glitch).
    let a = cycle_patch(mini_payload("c4"), Some(1), None);
    for _ in 0..120 {
        process_frame(&a);
    }
    assert!(
        seq_val(&a, "gate", 0) > 2.5,
        "c4 should be sounding before the swap"
    );

    let b = cycle_patch(mini_payload("c4"), Some(1), None);
    transfer_into(&b, &a);

    let mut prev_hi = false;
    let mut new_onsets = 0;
    let mut gate_dropped = false;
    for _ in 0..100 {
        // rest of cycle 0 — the same note continues
        process_frame(&b);
        let hi = seq_val(&b, "trig", 0) > 2.5;
        if hi && !prev_hi {
            new_onsets += 1;
        }
        prev_hi = hi;
        if seq_val(&b, "gate", 0) < 2.5 {
            gate_dropped = true;
        }
    }
    assert_eq!(
        new_onsets, 0,
        "a continued note must not re-trigger on swap"
    );
    assert!(
        !gate_dropped,
        "the gate must stay high across the swap (no glitch)"
    );
}
