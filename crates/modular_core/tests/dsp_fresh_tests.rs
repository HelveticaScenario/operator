//! Fresh integration tests for DSP modules.
//!
//! These tests verify that DSP modules produce correct audio output by
//! constructing modules via the public API, setting params as JSON, and
//! reading samples after ticking.

use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::DeserializedParams;
use modular_core::patch::Patch;
use modular_core::types::{ModuleState, PatchGraph, Sampleable};
use serde_json::json;

const SAMPLE_RATE: f32 = 48000.0;
const DEFAULT_PORT: &str = "output";

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
    )
    .unwrap_or_else(|e| panic!("constructor for '{module_type}' failed: {e}"))
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
    for _ in 0..500 {
        step(osc2.as_ref());
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
    for _ in 0..500 {
        step(sas.as_ref());
    }
    let sample = sas.get_poly_sample(DEFAULT_PORT).unwrap().get(0);

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
        "$curve" => json!({ "input": 0.0, "exp": 1.0 }),
        "$cycle" | "$intervalSeq" => json!({ "pattern": "0" }),
        "$iCycle" => json!({ "patterns": "0", "scale": "c(major)" }),
        "$slew" | "$quantizer" | "$unison" | "$crush" | "$feedback" | "$pulsar" | "$rising"
        | "$falling" | "$stereoMix" => json!({ "input": 0.0 }),
        "$track" => json!({ "keyframes": [] }),
        "$math" => json!({ "expression": "1+1" }),
        "$spread" => json!({ "min": -1.0, "max": 1.0, "count": 3 }),
        "$signal" => json!({ "source": 0.0 }),
        "$scaleAndShift" => json!({ "input": 0.0 }),
        "$cheby" | "$fold" | "$segment" => json!({ "input": 0.0, "amount": 0.0 }),
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
        let params = minimal_params(name);
        let cached = deserializer(params)
            .unwrap_or_else(|e| panic!("params deserialization for '{name}' failed: {e}"));
        let deserialized = DeserializedParams {
            params: cached.params,
            argument_spans: Default::default(),
            channel_count: cached.channel_count,
        };
        let module = constructor(&format!("test-{name}"), SAMPLE_RATE, deserialized).unwrap();
        // Should not panic with minimal params
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
    for _ in 0..500 {
        step(m.as_ref());
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
    let m = make_module("$curve", "curve-2", json!({ "input": 5.0, "exp": 3.0 }));
    for _ in 0..500 {
        step(m.as_ref());
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
    let m = make_module("$curve", "curve-3", json!({ "input": 2.5, "exp": 3.0 }));
    for _ in 0..500 {
        step(m.as_ref());
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
    let m = make_module("$curve", "curve-4", json!({ "input": -2.5, "exp": 2.0 }));
    for _ in 0..500 {
        step(m.as_ref());
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
    let m = make_module("$curve", "curve-5", json!({ "input": 0.0, "exp": 3.0 }));
    for _ in 0..500 {
        step(m.as_ref());
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
    let m = make_module("$curve", "curve-6", json!({ "input": 1.0, "exp": 0.0 }));
    for _ in 0..500 {
        step(m.as_ref());
    }
    let sample = m.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

    // 0.001s delay at 48 kHz = 48 frames.
    // Process 500 frames so param smoothing converges and the buffer is well-filled.
    for _ in 0..500 {
        process_frame(&patch);
    }

    let delay_module = patch.sampleables.get("delay").unwrap();
    let sample = delay_module.get_poly_sample(DEFAULT_PORT).unwrap().get(0);

    assert!(
        (sample - 2.0).abs() < 0.1,
        "delay read should output ~2.0 (constant input after filling), got {sample}"
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
    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

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
        let buf_sample = buf_module.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
        let delay_sample = delay_module.get_poly_sample(DEFAULT_PORT).unwrap().get(0);
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

    let old_patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

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
        .get_poly_sample(DEFAULT_PORT)
        .unwrap()
        .get(0);
    let old_b_output = old_patch
        .sampleables
        .get("b")
        .unwrap()
        .get_poly_sample(DEFAULT_PORT)
        .unwrap()
        .get(0);

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
    let new_patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

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
        .get_poly_sample(DEFAULT_PORT)
        .unwrap()
        .get(0);
    let new_b_output = new_patch
        .sampleables
        .get("b")
        .unwrap()
        .get_poly_sample(DEFAULT_PORT)
        .unwrap()
        .get(0);

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

// ─── IntervalSeq CV hold during rest after state transfer ────────────────────

#[test]
fn interval_seq_cv_holds_during_rest_after_state_transfer() {
    // Bug: After patch update (state transfer), $iCycle CV output drops to 0V
    // during rest periods. The sequencer only writes CV when a voice is active.
    // After reconstruction, the inner outputs default to 0.0. During a rest
    // cycle, no voice is active, so CV is never written and stays at 0.0 instead
    // of holding the last active note's voltage.
    //
    // Setup: $iCycle with pattern '<0 ~>' in d#(min) — alternates between
    // degree 0 (D#4 = 0.25V) and rest every cycle. Connect to a ROOT_CLOCK
    // with a high tempo so we can advance cycles quickly.
    //
    // At 48000 BPM with 4/4 time: one bar = 240 samples at 48kHz.
    //   Cycle 0 (samples 0-239): degree 0, CV = 0.25V (D#4)
    //   Cycle 1 (samples 240-479): rest, CV should HOLD at 0.25V
    //
    // Transfer state during cycle 1 (rest period), run one frame, verify CV != 0.

    let graph = make_graph(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 48000.0, "numerator": 4, "denominator": 4 }),
        ),
        (
            "seq",
            "$iCycle",
            json!({ "patterns": "<0 ~>", "scale": "d#(min)" }),
        ),
    ]);

    let old_patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

    // Advance through cycle 0 (degree 0) into the start of cycle 1 (rest).
    // One bar = 240 samples. Process 260 samples to be well into cycle 1.
    for _ in 0..260 {
        process_frame(&old_patch);
    }

    // Check that the CV output holds the last active voltage (not zero).
    // D#4 in V/Oct: (63 - 60) / 12 = 0.25V
    let expected_voltage = 0.25f32;

    let old_cv = old_patch
        .sampleables
        .get("seq")
        .unwrap()
        .get_poly_sample("cv")
        .unwrap()
        .get(0);

    // During rest, the old module should still hold the last active voltage.
    // (This verifies the test setup is correct — the old module works fine.)
    assert!(
        (old_cv - expected_voltage).abs() < 0.01,
        "old module CV should hold {expected_voltage}V during rest, got {old_cv}"
    );

    // Now simulate a force update: build new patch, transfer state, connect.
    let new_patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("from_graph failed");

    for (id, new_module) in &new_patch.sampleables {
        if let Some(old_module) = old_patch.sampleables.get(id) {
            new_module.transfer_state_from(old_module.as_ref());
        }
    }

    for module in new_patch.sampleables.values() {
        module.connect(&new_patch);
    }

    // Call on_patch_update (as apply_patch_update does)
    for module in new_patch.sampleables.values() {
        module.on_patch_update();
    }

    // Run ONE frame on the new patch — still in the rest period.
    process_frame(&new_patch);

    let new_cv = new_patch
        .sampleables
        .get("seq")
        .unwrap()
        .get_poly_sample("cv")
        .unwrap()
        .get(0);

    // The CV should still hold the previous active voltage, not drop to 0.
    assert!(
        (new_cv - expected_voltage).abs() < 0.01,
        "after state transfer during rest, CV should hold {expected_voltage}V, got {new_cv}\n\
         (0.0 means inner outputs were not preserved across state transfer)"
    );
}
