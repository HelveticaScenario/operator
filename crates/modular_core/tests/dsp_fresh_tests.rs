//! Fresh integration tests for DSP modules.
//!
//! These tests verify that DSP modules produce correct audio output by
//! constructing modules via the public API, setting params as JSON, and
//! reading samples after ticking.

use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::{extract_argument_spans, DeserializedParams};
use modular_core::patch::Patch;
use modular_core::types::{ModuleState, PatchGraph, Sampleable};
use serde_json::json;
use std::sync::Arc;

const SAMPLE_RATE: f32 = 48000.0;
const DEFAULT_PORT: &str = "output";

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Create a named module from the constructor registry with default params.
fn make_module(module_type: &str, id: &str) -> Arc<Box<dyn Sampleable>> {
    let constructors = get_constructors();
    let deserializers = get_params_deserializers();
    let deserializer = deserializers
        .get(module_type)
        .unwrap_or_else(|| panic!("no params deserializer for '{module_type}'"));
    let cached = deserializer(json!({})).unwrap_or_else(|e| {
        panic!("default params deserialization for '{module_type}' failed: {e}")
    });
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
    )
    .unwrap_or_else(|e| panic!("constructor for '{module_type}' failed: {e}"))
}

/// Set params on a module (JSON → deserialize → apply_deserialized_params).
fn set_params(module: &dyn Sampleable, params: serde_json::Value, _channels: usize) {
    let deserializers = get_params_deserializers();
    let module_type = module.get_module_type();
    let deserializer = deserializers
        .get(module_type)
        .unwrap_or_else(|| panic!("no params deserializer for '{module_type}'"));
    let (stripped, argument_spans) = extract_argument_spans(params);
    let cached = deserializer(stripped).expect("params deserialization failed");
    let deserialized = DeserializedParams {
        params: cached.params,
        argument_spans,
        channel_count: cached.channel_count,
    };
    module
        .apply_deserialized_params(deserialized)
        .expect("apply_deserialized_params failed");
}

/// Advance one sample: tick then update.
fn step(module: &dyn Sampleable) {
    module.tick();
    module.update();
}

/// Advance N samples and collect the first channel of `output`.
fn collect_samples(module: &dyn Sampleable, n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        step(module);
        let sample = module
            .get_poly_sample(DEFAULT_PORT)
            .expect("get_poly_sample failed")
            .get(0);
        out.push(sample);
    }
    out
}

/// Collect N samples from a specific channel.
fn collect_channel(module: &dyn Sampleable, channel: usize, n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        step(module);
        let sample = module
            .get_poly_sample(DEFAULT_PORT)
            .expect("get_poly_sample failed")
            .get(channel);
        out.push(sample);
    }
    out
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
    let osc = make_module("$sine", "sine-1");
    // 0 V/oct ≈ C4 (261.63 Hz)
    set_params(&**osc, json!({ "freq": 0.0 }), 1);

    let samples = collect_samples(&**osc, 1000);
    let (mn, mx) = min_max(&samples);

    // Sine output should swing ±5 V
    assert!(mx > 4.5, "peak should be close to +5V, got {mx}");
    assert!(mn < -4.5, "trough should be close to -5V, got {mn}");
}

#[test]
fn sine_zero_frequency_is_dc() {
    let osc = make_module("$sine", "sine-1");
    // Very low frequency → nearly DC over 100 samples
    set_params(&**osc, json!({ "freq": -10.0 }), 1);

    let samples = collect_samples(&**osc, 100);
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
    let osc = make_module("$sine", "sine-1");
    // Two channels at different pitches
    set_params(&**osc, json!({ "freq": [0.0, 1.0] }), 2);

    let _ch0 = collect_channel(&**osc, 0, 500);
    // Reset for channel 1 read — we already stepped, so just read accumulated data
    // Actually the module already computed both channels per tick.
    // We need to re-check: collect_channel steps the module, so ch1 will be
    // from subsequent samples. That's fine — we just want to verify both channels
    // produce output.

    let osc2 = make_module("$sine", "sine-2");
    set_params(&**osc2, json!({ "freq": [0.0, 1.0] }), 2);

    let mut ch0_samples = Vec::new();
    let mut ch1_samples = Vec::new();
    for _ in 0..500 {
        step(&**osc2);
        let poly = osc2.get_poly_sample(DEFAULT_PORT).unwrap();
        ch0_samples.push(poly.get(0));
        ch1_samples.push(poly.get(1));
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
    let osc = make_module("$saw", "saw-1");
    set_params(&**osc, json!({ "freq": 0.0 }), 1);

    let samples = collect_samples(&**osc, 1000);
    let (mn, mx) = min_max(&samples);

    assert!(mx > 4.0, "saw peak should be near +5V, got {mx}");
    assert!(mn < -4.0, "saw trough should be near -5V, got {mn}");
}

// ─── Pulse oscillator ────────────────────────────────────────────────────────

#[test]
fn pulse_produces_bipolar_output() {
    let osc = make_module("$pulse", "pulse-1");
    set_params(&**osc, json!({ "freq": 0.0 }), 1);

    let samples = collect_samples(&**osc, 1000);
    let (mn, mx) = min_max(&samples);

    assert!(mx > 4.0, "pulse peak should be near +5V, got {mx}");
    assert!(mn < -4.0, "pulse trough should be near -5V, got {mn}");
}

#[test]
fn pulse_width_affects_duty_cycle() {
    // Width 0 → near 50/50, width 5 → narrower positive
    let osc_narrow = make_module("$pulse", "pulse-narrow");
    set_params(&**osc_narrow, json!({ "freq": 0.0, "width": 4.0 }), 1);
    let samples_narrow = collect_samples(&**osc_narrow, 1000);

    let osc_wide = make_module("$pulse", "pulse-wide");
    set_params(&**osc_wide, json!({ "freq": 0.0, "width": 0.0 }), 1);
    let samples_wide = collect_samples(&**osc_wide, 1000);

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
    let n = make_module("$noise", "noise-1");
    set_params(&**n, json!({ "color": "white" }), 1);

    let samples = collect_samples(&**n, 1000);
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
    let sas = make_module("$scaleAndShift", "sas-1");
    // Formula: output = input * (scale / 5.0) + shift
    // input=1.0, scale=5.0 (= 1x gain), shift=2.0 → output = 1.0 * 1.0 + 2.0 = 3.0
    set_params(
        &**sas,
        json!({ "input": 1.0, "scale": 5.0, "shift": 2.0 }),
        1,
    );

    // Step enough times for param smoothing to converge
    for _ in 0..500 {
        step(&**sas);
    }
    let sample = sas.get_poly_sample(DEFAULT_PORT).unwrap().get(0);

    assert!(approx_eq(sample, 3.0, 0.1), "expected ~3.0, got {sample}");
}

// ─── Constructors ────────────────────────────────────────────────────────────

#[test]
fn all_constructors_produce_valid_modules() {
    let constructors = get_constructors();
    let deserializers = get_params_deserializers();
    for (name, constructor) in &constructors {
        let deserializer = deserializers
            .get(name.as_str())
            .unwrap_or_else(|| panic!("no params deserializer for '{name}'"));
        let cached = deserializer(json!({}))
            .unwrap_or_else(|e| panic!("default params deserialization for '{name}' failed: {e}"));
        let deserialized = DeserializedParams {
            params: cached.params,
            argument_spans: Default::default(),
            channel_count: cached.channel_count,
        };
        let module = constructor(&format!("test-{name}"), SAMPLE_RATE, deserialized);
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
        let cached = deserializer(json!({}))
            .unwrap_or_else(|e| panic!("default params deserialization for '{name}' failed: {e}"));
        let deserialized = DeserializedParams {
            params: cached.params,
            argument_spans: Default::default(),
            channel_count: cached.channel_count,
        };
        let module = constructor(&format!("test-{name}"), SAMPLE_RATE, deserialized).unwrap();
        // Should not panic with default (zero) params
        module.tick();
        module.update();
        let _ = module.get_poly_sample(DEFAULT_PORT);
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

// ─── Param validation ────────────────────────────────────────────────────────

#[test]
fn param_validators_accept_valid_params() {
    use modular_core::dsp::get_param_validators;
    let validators = get_param_validators();

    // Sine with a numeric freq should pass
    if let Some(validate) = validators.get("$sine") {
        let result = validate(&json!({ "freq": 0.0 }));
        assert!(result.is_ok(), "valid sine params rejected: {:?}", result);
    }
}

#[test]
fn param_validators_reject_bogus_params() {
    use modular_core::dsp::get_param_validators;
    let validators = get_param_validators();

    // Sine with an object as freq should fail
    if let Some(validate) = validators.get("$sine") {
        let result = validate(&json!({ "freq": { "nested": true } }));
        assert!(result.is_err(), "invalid sine params should be rejected");
    }
}

// ─── Patch-level helpers ─────────────────────────────────────────────────────

/// Process one frame of the entire patch (update all, then tick all).
/// Mirrors the ordering in `AudioThread::process_frame`.
fn process_frame(patch: &Patch) {
    for module in patch.sampleables.values() {
        module.update();
    }
    for module in patch.sampleables.values() {
        module.tick();
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
    }
}

// ─── from_graph integration tests ────────────────────────────────────────────

#[test]
fn from_graph_creates_patch_with_modules() {
    let graph = make_graph(vec![
        ("osc1", "$sine", json!({ "freq": 0.0 })),
        ("osc2", "$saw", json!({ "freq": 1.0 })),
    ]);
    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

    // Both oscillators plus the hidden AudioIn
    assert!(patch.sampleables.contains_key("osc1"));
    assert!(patch.sampleables.contains_key("osc2"));
    assert!(patch.sampleables.contains_key("HIDDEN_AUDIO_IN"));
}

#[test]
fn from_graph_rejects_unknown_module_type() {
    let graph = make_graph(vec![("bad", "$nonexistent", json!({}))]);
    let result = Patch::from_graph(&graph, SAMPLE_RATE);
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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

    // Let param smoothing converge
    for _ in 0..500 {
        process_frame(&patch);
    }

    let module = patch.sampleables.get("sas1").unwrap();
    let sample = module.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

    // Collect samples from the $signal module — it should reproduce the sine output
    let sig_module = patch.sampleables.get("sig").unwrap();
    let osc_module = patch.sampleables.get("osc").unwrap();

    let mut sig_samples = Vec::new();
    let mut osc_samples = Vec::new();
    for _ in 0..1000 {
        process_frame(&patch);
        sig_samples.push(sig_module.get_poly_sample(DEFAULT_PORT).unwrap().get(0));
        osc_samples.push(osc_module.get_poly_sample(DEFAULT_PORT).unwrap().get(0));
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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

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
    let direct_patch = Patch::from_graph(&direct_graph, SAMPLE_RATE).expect("from_graph failed");

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
        filtered.push(mix_filtered.get_poly_sample(DEFAULT_PORT).unwrap().get(0));
        direct.push(mix_direct.get_poly_sample(DEFAULT_PORT).unwrap().get(0));
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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

    for _ in 0..500 {
        process_frame(&patch);
    }

    let fast = patch.sampleables.get("fast").unwrap();
    let slow = patch.sampleables.get("slow").unwrap();

    let mut fast_samples = Vec::new();
    let mut slow_samples = Vec::new();
    for _ in 0..2000 {
        process_frame(&patch);
        fast_samples.push(fast.get_poly_sample(DEFAULT_PORT).unwrap().get(0));
        slow_samples.push(slow.get_poly_sample(DEFAULT_PORT).unwrap().get(0));
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

// ─── Curve ───────────────────────────────────────────────────────────────────

#[test]
fn curve_linear_passthrough() {
    // exp=1 should be linear: output ≈ input
    let m = make_module("$curve", "curve-1");
    set_params(&**m, json!({ "input": 3.0, "exp": 1.0 }), 1);
    for _ in 0..500 {
        step(&**m);
    }
    let sample = m.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
    assert!(
        approx_eq(sample, 3.0, 0.1),
        "exp=1 should pass through, got {sample}"
    );
}

#[test]
fn curve_unity_at_5v() {
    // At 5V input, output should be 5V regardless of exponent
    let m = make_module("$curve", "curve-2");
    set_params(&**m, json!({ "input": 5.0, "exp": 3.0 }), 1);
    for _ in 0..500 {
        step(&**m);
    }
    let sample = m.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
    assert!(
        approx_eq(sample, 5.0, 0.1),
        "5V should stay 5V, got {sample}"
    );
}

#[test]
fn curve_cubic_midpoint() {
    // exp=3, input=2.5: output = 5 * (2.5/5)^3 = 5 * 0.125 = 0.625
    let m = make_module("$curve", "curve-3");
    set_params(&**m, json!({ "input": 2.5, "exp": 3.0 }), 1);
    for _ in 0..500 {
        step(&**m);
    }
    let sample = m.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
    assert!(
        approx_eq(sample, 0.625, 0.1),
        "expected ~0.625, got {sample}"
    );
}

#[test]
fn curve_preserves_sign() {
    // Negative input should produce negative output
    let m = make_module("$curve", "curve-4");
    set_params(&**m, json!({ "input": -2.5, "exp": 2.0 }), 1);
    for _ in 0..500 {
        step(&**m);
    }
    let sample = m.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
    // sign(-2.5) * 5 * (2.5/5)^2 = -1 * 5 * 0.25 = -1.25
    assert!(
        approx_eq(sample, -1.25, 0.1),
        "expected ~-1.25, got {sample}"
    );
}

#[test]
fn curve_zero_input() {
    // Zero input should produce zero output
    let m = make_module("$curve", "curve-5");
    set_params(&**m, json!({ "input": 0.0, "exp": 3.0 }), 1);
    for _ in 0..500 {
        step(&**m);
    }
    let sample = m.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
    assert!(
        approx_eq(sample, 0.0, 0.01),
        "0V input should produce 0V, got {sample}"
    );
}

#[test]
fn curve_exp_zero_step_function() {
    // exp=0: any nonzero input → ±5V
    let m = make_module("$curve", "curve-6");
    set_params(&**m, json!({ "input": 1.0, "exp": 0.0 }), 1);
    for _ in 0..500 {
        step(&**m);
    }
    let sample = m.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
    assert!(
        approx_eq(sample, 5.0, 0.1),
        "exp=0 nonzero input should → 5V, got {sample}"
    );
}
