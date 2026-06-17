//! Fresh integration tests for DSP modules.
//!
//! These tests verify that DSP modules produce correct audio output by
//! constructing modules via the public API, setting params as JSON, and
//! reading samples after ticking.

use std::collections::HashMap;

use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::DeserializedParams;
use modular_core::patch::Patch;
use modular_core::types::{ModuleState, PatchGraph, Sampleable};
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
/// would build the map via `modular::graph_analysis::classify_modules`
/// first, but the patches here are acyclic.
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
        argument_spans: Default::default(),
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
        "$slew" | "$dcBlock" | "$quantizer" | "$unison" | "$crush" | "$feedback" | "$pulsar"
        | "$rising" | "$falling" | "$stereoMix" => json!({ "input": 0.0 }),
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
            argument_spans: Default::default(),
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
            argument_spans: Default::default(),
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
            .map(|(id, module_type, params)| ModuleState {
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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let direct_patch = Patch::from_graph(&direct_graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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

    let old_patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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
    let new_patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("from_graph failed");

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

/// Sum the number of highlight spans across every `pattern.*` (or `pattern`)
/// key in a `get_state()` param_spans object. Returns `None` if the module
/// produced no state at all.
fn total_highlight_spans(state: &Option<Value>) -> Option<usize> {
    let state = state.as_ref()?;
    let param_spans = state.get("param_spans")?.as_object()?;
    let mut total = 0;
    for (_key, entry) in param_spans {
        if let Some(spans) = entry.get("spans").and_then(|s| s.as_array()) {
            total += spans.len();
        }
    }
    Some(total)
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
    // single-source pattern) into the new module. on_patch_update clears
    // current_cycle + module_cache but NOT voices[].cached_hap.
    //
    // The held voice still satisfies cached.contains(playhead) off its scalar
    // whole_begin/whole_end, so it is not released and no fresh onset fires
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
    let old_patch =
        Patch::from_graph(&graph_a, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("A from_graph failed");

    // One bar = 240 samples. The trailing `5` covers 0.5..1.0 (samples
    // 120..240). Advance ~180 samples so the playhead sits at ~0.75, latching
    // the `5` hap (geometry whole_begin=0.5, whole_end=1.0).
    for _ in 0..180 {
        process_frame(&old_patch);
    }

    // Sanity: the OLD single-source module reports a non-empty highlight.
    let old_state = old_patch.sampleables.get("seq").unwrap().get_state();
    let old_spans = total_highlight_spans(&old_state);
    assert!(
        matches!(old_spans, Some(n) if n > 0),
        "old single-source module should highlight the held `5` step; got state={old_state:?}"
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
    let new_patch =
        Patch::from_graph(&graph_b, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("B from_graph failed");

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
    let healed_state = seq_b.get_state();
    let healed_spans = total_highlight_spans(&healed_state);
    let healed_keys: Vec<String> = healed_state
        .as_ref()
        .and_then(|s| s.get("param_spans"))
        .and_then(|p| p.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    eprintln!("--- seq highlight transfer (self-heal) ---");
    eprintln!("old (single-source) total spans = {old_spans:?}");
    eprintln!("after transfer, param_spans keys = {healed_keys:?}");
    eprintln!("after transfer, total spans      = {healed_spans:?}");
    eprintln!("healed state = {healed_state:?}");

    // The fix: immediately after transfer (no restart), with a voice still
    // held off a stale/out-of-range hap_index, the highlight read self-heals
    // by geometry and surfaces the spans for the step actually playing.
    assert!(
        matches!(healed_spans, Some(n) if n > 0),
        "after state transfer with a held voice, the highlight should self-heal \
         (re-resolve the stale hap_index by geometry) and be NON-EMPTY. \
         Got total={healed_spans:?}, keys={healed_keys:?}, state={healed_state:?}"
    );

    // Multi-source: both pattern.0 (`5`) and pattern.1 (`4`) contributed to the
    // held step, so both keys must carry spans.
    let param_spans = healed_state
        .as_ref()
        .and_then(|s| s.get("param_spans"))
        .and_then(|p| p.as_object())
        .expect("param_spans map present");
    for key in ["pattern.0", "pattern.1"] {
        let spans = param_spans
            .get(key)
            .and_then(|e| e.get("spans"))
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("missing {key} spans: {param_spans:?}"));
        assert!(
            !spans.is_empty(),
            "{key} should carry spans for the held step after self-heal: {param_spans:?}"
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
    let ctrl_patch =
        Patch::from_graph(&graph_c, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new()).expect("C from_graph failed");
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
    let ctrl_spans = total_highlight_spans(&ctrl_patch.sampleables.get("seq").unwrap().get_state());
    assert!(
        matches!(ctrl_spans, Some(n) if n > 0),
        "equal-geometry single-source re-run should keep highlight (index still \
         valid, fast path); got {ctrl_spans:?}"
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

// ─── DC blocker ($dcBlock) ───────────────────────────────────────────────────

/// Helper: compute the mean (DC component) of a sample buffer.
fn dc_offset(samples: &[f32]) -> f32 {
    samples.iter().sum::<f32>() / samples.len() as f32
}

#[test]
fn dc_block_removes_constant_offset_without_transient() {
    // A pure DC input is the degenerate case: priming the input history means
    // the very first output is already 0 and stays there — offset fully
    // removed, no start-up transient.
    let m = make_module("$dcBlock", "dc-1", json!({ "input": 2.5 }));
    let samples = collect_samples(&*m, 2000);
    for (i, s) in samples.iter().enumerate() {
        assert!(
            s.abs() < 1e-3,
            "constant 2.5V input should block to ~0V every sample; sample {i} = {s}V"
        );
    }
}

#[test]
fn dc_block_centers_offset_pulse_and_preserves_ac() {
    // A 20%-duty pulse sits well below 0V on average; $dcBlock should recenter
    // it on 0V while leaving the ~10V peak-to-peak audio swing intact.
    let graph = make_graph(vec![
        ("osc", "$pulse", json!({ "freq": 0.0, "width": 1.0 })),
        (
            "dc",
            "$dcBlock",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 }
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");

    let osc = patch.sampleables.get("osc").unwrap();
    let dc = patch.sampleables.get("dc").unwrap();

    // Settle past the blocker's ~8 ms time constant before measuring.
    for _ in 0..2000 {
        process_frame(&patch);
    }

    let mut raw = Vec::new();
    let mut blocked = Vec::new();
    for _ in 0..10000 {
        process_frame(&patch);
        raw.push(osc.get_value_at(DEFAULT_PORT, 0, 0));
        blocked.push(dc.get_value_at(DEFAULT_PORT, 0, 0));
    }

    // The raw pulse carries a clearly negative DC offset…
    let raw_dc = dc_offset(&raw);
    assert!(
        raw_dc < -1.0,
        "20% pulse should sit well below 0V; raw DC = {raw_dc}V"
    );

    // …which $dcBlock removes.
    let blocked_dc = dc_offset(&blocked);
    assert!(
        blocked_dc.abs() < 0.3,
        "$dcBlock should recenter the pulse on 0V; blocked DC = {blocked_dc}V"
    );

    // AC content survives: peak-to-peak stays ~10V.
    let (mn, mx) = min_max(&blocked);
    assert!(
        mx - mn > 9.0,
        "$dcBlock should preserve the ~10V swing; got {}V (min={mn}, max={mx})",
        mx - mn
    );

    // The waveform is *passed*, not differentiated to edge spikes: most samples
    // sit at a steady level, so RMS stays high (~4V for this duty cycle). A
    // degenerate coefficient (e.g. a skipped init leaving R = 0) would collapse
    // the output to a near-silent spike train.
    let rms = (blocked.iter().map(|s| s * s).sum::<f32>() / blocked.len() as f32).sqrt();
    assert!(
        rms > 2.0,
        "$dcBlock should pass the square waveform (high RMS), not differentiate it; RMS = {rms}V"
    );
}

// ─── Virtual range ports + dynamic_range ─────────────────────────────────────

#[test]
fn utility_modules_have_dynamic_range_in_schema() {
    use modular_core::dsp::schema;
    let schemas = schema();
    for module_name in ["$remap", "$wrap", "$spread", "$scaleAndShift", "$clamp"] {
        let s = schemas
            .iter()
            .find(|s| s.name == module_name)
            .unwrap_or_else(|| panic!("missing schema for {module_name}"));
        let output = s
            .outputs
            .iter()
            .find(|o| o.default)
            .unwrap_or_else(|| panic!("{module_name} has no default output"));
        assert!(
            output.dynamic_range,
            "{module_name} default output should be dynamic_range"
        );
    }
}

#[test]
fn static_range_output_exposes_virtual_range_ports() {
    // $sine has static range = (-5, 5). Virtual ports must still resolve and
    // return the compile-time constants, even though no `dynamic_range`
    // BlockPort is allocated.
    let osc = make_module("$sine", "sine", json!({ "freq": 440.0 }));
    osc.start_block();
    osc.ensure_processed();
    let min = osc.get_value_at("output.rangeMin", 0, 0);
    let max = osc.get_value_at("output.rangeMax", 0, 0);
    assert!((min - (-5.0)).abs() < 0.01, "static rangeMin should be -5, got {min}");
    assert!((max - 5.0).abs() < 0.01, "static rangeMax should be 5, got {max}");
}

#[test]
fn sampleable_get_range_returns_constants_for_static_range() {
    let osc = make_module("$sine", "sine", json!({ "freq": 0.0 }));
    osc.start_block();
    osc.ensure_processed();
    let r = osc.get_range("output", 0, 0);
    assert!(r.is_some(), "static-range output should expose get_range");
    let (min, max) = r.unwrap();
    assert!((min - (-5.0)).abs() < 0.01, "rangeMin should be -5, got {min}");
    assert!((max - 5.0).abs() < 0.01, "rangeMax should be 5, got {max}");
}

#[test]
fn signal_get_range_returns_none_for_volts() {
    use modular_core::types::Signal;
    assert!(Signal::Volts(3.0).get_range().is_none());
}

#[test]
fn remap_dynamic_range_tracks_outMin_outMax() {
    let m = make_module(
        "$remap",
        "remap",
        json!({ "input": 0.0, "inMin": -5.0, "inMax": 5.0, "outMin": -3.0, "outMax": 7.0 }),
    );
    for _ in 0..1000 {
        Stepper::new().tick(&*m);
    }
    let (min, max) = m.get_range("output", 0, 0).unwrap();
    assert!((min - (-3.0)).abs() < 0.1, "remap rangeMin should be ~-3, got {min}");
    assert!((max - 7.0).abs() < 0.1, "remap rangeMax should be ~7, got {max}");
}

#[test]
fn wrap_dynamic_range_uses_min_max() {
    let m = make_module(
        "$wrap",
        "wrap",
        json!({ "input": 6.0, "min": 1.0, "max": 4.0 }),
    );
    m.start_block();
    m.ensure_processed();
    let (min, max) = m.get_range("output", 0, 0).unwrap();
    assert!((min - 1.0).abs() < 0.01, "wrap rangeMin should be 1, got {min}");
    assert!((max - 4.0).abs() < 0.01, "wrap rangeMax should be 4, got {max}");
}

#[test]
fn wrap_dynamic_range_swaps_when_max_lt_min() {
    let m = make_module(
        "$wrap",
        "wrap",
        json!({ "input": 0.0, "min": 5.0, "max": 0.0 }),
    );
    m.start_block();
    m.ensure_processed();
    let (min, max) = m.get_range("output", 0, 0).unwrap();
    assert!((min - 0.0).abs() < 0.01, "wrap rangeMin should be 0 (swapped), got {min}");
    assert!((max - 5.0).abs() < 0.01, "wrap rangeMax should be 5 (swapped), got {max}");
}

#[test]
fn spread_dynamic_range_uses_min_max() {
    let m = make_module(
        "$spread",
        "spread",
        json!({ "min": 2.0, "max": 8.0, "count": 4 }),
    );
    m.start_block();
    m.ensure_processed();
    let (min, max) = m.get_range("output", 0, 0).unwrap();
    assert!((min - 2.0).abs() < 0.01, "spread rangeMin should be 2, got {min}");
    assert!((max - 8.0).abs() < 0.01, "spread rangeMax should be 8, got {max}");
}

#[test]
fn scale_and_shift_dynamic_range_from_input() {
    // $sine([-5,5]) → $scaleAndShift(scale=5, shift=1):
    // unity gain + 1V offset → range [-4, 6].
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 0.0 })),
        (
            "sas",
            "$scaleAndShift",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 },
                "scale": 5.0,
                "shift": 1.0
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");
    for _ in 0..200 {
        process_frame(&patch);
    }
    let sas = patch.sampleables.get("sas").unwrap();
    let (min, max) = sas.get_range("output", 0, 0).unwrap();
    assert!((min - (-4.0)).abs() < 0.1, "rangeMin should be ~-4, got {min}");
    assert!((max - 6.0).abs() < 0.1, "rangeMax should be ~6, got {max}");
}

#[test]
fn clamp_dynamic_range_intersects_with_input() {
    // $sine([-5,5]) → $clamp(min=-2, max=3): intersection [-2, 3].
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 0.0 })),
        (
            "cl",
            "$clamp",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 },
                "min": -2.0,
                "max": 3.0
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");
    for _ in 0..200 {
        process_frame(&patch);
    }
    let cl = patch.sampleables.get("cl").unwrap();
    let (min, max) = cl.get_range("output", 0, 0).unwrap();
    assert!((min - (-2.0)).abs() < 0.1, "rangeMin should be ~-2, got {min}");
    assert!((max - 3.0).abs() < 0.1, "rangeMax should be ~3, got {max}");
}

#[test]
fn remap_dynamic_range_swaps_when_out_min_gt_out_max() {
    // An inverted remap (`outMin > outMax`) is a legitimate inversion that
    // map_range handles. The published range must still be ordered
    // (rangeMin <= rangeMax), matching $wrap / $spread / $scaleAndShift.
    let m = make_module(
        "$remap",
        "remap",
        json!({ "input": 0.0, "inMin": -5.0, "inMax": 5.0, "outMin": 5.0, "outMax": -5.0 }),
    );
    for _ in 0..1000 {
        Stepper::new().tick(&*m);
    }
    let (min, max) = m.get_range("output", 0, 0).unwrap();
    assert!((min - (-5.0)).abs() < 0.1, "remap rangeMin should be -5 (ordered), got {min}");
    assert!((max - 5.0).abs() < 0.1, "remap rangeMax should be 5 (ordered), got {max}");
}

#[test]
fn scale_and_shift_dynamic_range_negative_gain() {
    // $sine([-5,5]) → $scaleAndShift(scale=-5, shift=0): g=-1 flips the
    // bounds, so the published range must be reordered to [-5, 5], not [5, -5].
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 0.0 })),
        (
            "sas",
            "$scaleAndShift",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 },
                "scale": -5.0,
                "shift": 0.0
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");
    for _ in 0..200 {
        process_frame(&patch);
    }
    let sas = patch.sampleables.get("sas").unwrap();
    let (min, max) = sas.get_range("output", 0, 0).unwrap();
    assert!((min - (-5.0)).abs() < 0.1, "rangeMin should be ~-5 (reordered), got {min}");
    assert!((max - 5.0).abs() < 0.1, "rangeMax should be ~5 (reordered), got {max}");
}

#[test]
fn clamp_dynamic_range_orders_inverted_bounds() {
    // $sine([-5,5]) → $clamp(min=3, max=-2): the clamp value path orders the
    // bounds to [-2, 3], and the composed range must order them the same way
    // instead of computing lo=3 > hi=-2 and silently dropping the publish
    // (which would leave the static fallback (-5, 5)).
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 0.0 })),
        (
            "cl",
            "$clamp",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 },
                "min": 3.0,
                "max": -2.0
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");
    for _ in 0..200 {
        process_frame(&patch);
    }
    let cl = patch.sampleables.get("cl").unwrap();
    let (min, max) = cl.get_range("output", 0, 0).unwrap();
    assert!((min - (-2.0)).abs() < 0.1, "clamp rangeMin should be ~-2 (ordered), got {min}");
    assert!((max - 3.0).abs() < 0.1, "clamp rangeMax should be ~3 (ordered), got {max}");
}

#[test]
fn dynamic_range_is_read_at_the_consumer_sample_slot() {
    // At block_size > 1 a consumer must read an upstream range at ITS sample
    // slot, not the producer's latest slot. Drive $scaleAndShift's `scale`
    // with a per-sample-varying $saw so the composed output range differs
    // between slots 0 and 1, then assert each slot reads its own range. The
    // old block-granular read returned the last slot for both, so it would
    // see slot 1's range at slot 0 and fail.
    const BS: usize = 2;
    let graph = make_graph(vec![
        // Static range [-5, 5], slot-independent — isolates the variation to `scale`.
        ("in", "$sine", json!({ "freq": 0.0 })),
        // ~4.2 kHz saw (freq is V/Oct, 0 V = C4) advances ~0.087 cycle/sample,
        // so slot 0 and slot 1 carry distinct values.
        ("mod", "$saw", json!({ "freq": 4.0 })),
        (
            "sas",
            "$scaleAndShift",
            json!({
                "input": { "type": "cable", "module": "in", "port": "output", "channel": 0 },
                "scale": { "type": "cable", "module": "mod", "port": "output", "channel": 0 },
                "shift": 0.0
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, BS, &HashMap::new()).expect("from_graph failed");

    // Process one full BS-sample block.
    for m in patch.sampleables.values() {
        m.start_block();
    }
    for m in patch.sampleables.values() {
        m.ensure_processed();
    }

    let modu = patch.sampleables.get("mod").unwrap();
    let sas = patch.sampleables.get("sas").unwrap();

    // Expected composed range at a slot: input [-5, 5] scaled by g = scale/5
    // (reordered when g < 0), derived from the scale value actually produced.
    let expect = |slot: usize| {
        let g = modu.get_value_at("output", 0, slot) / 5.0;
        let (a, b) = (-5.0 * g, 5.0 * g);
        if a <= b { (a, b) } else { (b, a) }
    };
    let (e0min, e0max) = expect(0);
    let (e1min, e1max) = expect(1);

    let (r0min, r0max) = sas.get_range("output", 0, 0).expect("range at slot 0");
    let (r1min, r1max) = sas.get_range("output", 0, 1).expect("range at slot 1");

    assert!(
        (r0min - e0min).abs() < 1e-3 && (r0max - e0max).abs() < 1e-3,
        "slot 0 range {:?} should match scale@0 → {:?}",
        (r0min, r0max),
        (e0min, e0max)
    );
    assert!(
        (r1min - e1min).abs() < 1e-3 && (r1max - e1max).abs() < 1e-3,
        "slot 1 range {:?} should match scale@1 → {:?}",
        (r1min, r1max),
        (e1min, e1max)
    );
    // Guard: the two slots must actually differ, otherwise the assertions
    // above would pass even under the old block-granular read.
    assert!(
        (r0min - r1min).abs() > 1e-4 || (r0max - r1max).abs() > 1e-4,
        "per-slot ranges should differ; scale@0={}, scale@1={}",
        modu.get_value_at("output", 0, 0),
        modu.get_value_at("output", 0, 1)
    );
}

#[test]
fn clamp_remap_via_virtual_range_ports_end_to_end() {
    // Read a dynamic_range module's virtual range ports over a cable and feed
    // them into $remap's inMin / inMax — the cable-driven mirror of
    // `$clamp(sine, -2, 3).range(0, 1)`. $clamp publishes its bounded range
    // [-2, 3], so remapping that swing onto [0, 1] should span the unit range.
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 0.0 })),
        (
            "clamp",
            "$clamp",
            json!({
                "input": { "type": "cable", "module": "osc", "port": "output", "channel": 0 },
                "min": -2.0,
                "max": 3.0
            }),
        ),
        (
            "remap",
            "$remap",
            json!({
                "input":  { "type": "cable", "module": "clamp", "port": "output", "channel": 0 },
                "outMin": 0.0,
                "outMax": 1.0,
                "inMin":  { "type": "cable", "module": "clamp", "port": "output.rangeMin", "channel": 0 },
                "inMax":  { "type": "cable", "module": "clamp", "port": "output.rangeMax", "channel": 0 }
            }),
        ),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE, TEST_BLOCK_SIZE, &HashMap::new())
        .expect("from_graph failed");
    for _ in 0..2000 {
        process_frame(&patch);
    }
    let remap = patch.sampleables.get("remap").unwrap();
    let mut samples = Vec::new();
    for _ in 0..5000 {
        process_frame(&patch);
        samples.push(remap.get_value_at("output", 0, 0));
    }
    let (mn, mx) = min_max(&samples);
    assert!(mn >= -0.05, "remap output min should be >= 0, got {mn}");
    assert!(mx <= 1.05, "remap output max should be <= 1, got {mx}");
    assert!(mn < 0.15, "remap output should reach near 0, got {mn}");
    assert!(mx > 0.85, "remap output should reach near 1, got {mx}");
}
