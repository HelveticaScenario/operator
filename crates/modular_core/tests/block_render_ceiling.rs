//! The audio callback caps Block-mode rendering at the emit boundary via
//! `set_block_render_ceiling` so a mid-block patch swap transfers module
//! state that sits exactly at the swap point. These tests pin the wrapper
//! contract: reads never render past the ceiling, and rendering a block in
//! ceiling-bounded chunks produces the same samples as rendering it in one
//! greedy pass.

use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::DeserializedParams;
use modular_core::types::{Sampleable, set_block_render_ceiling};
use serde_json::json;

const SAMPLE_RATE: f32 = 48000.0;
const BLOCK_SIZE: usize = 64;

fn make_module(module_type: &str, id: &str, params: serde_json::Value) -> Box<dyn Sampleable> {
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
    get_constructors()
        .get(module_type)
        .unwrap_or_else(|| panic!("no constructor for '{module_type}'"))(
        &id.to_string(),
        SAMPLE_RATE,
        deserialized,
        BLOCK_SIZE,
        modular_core::types::ProcessingMode::Block,
    )
    .unwrap_or_else(|e| panic!("constructor for '{module_type}' failed: {e}"))
}

/// A read below the ceiling must not render slots beyond it. The ceiling is
/// thread-local, so restore the permissive default before returning.
#[test]
fn read_does_not_render_past_ceiling() {
    let module = make_module("$signal", "sig", json!({ "source": 5.0 }));
    module.start_block();

    set_block_render_ceiling(8);
    assert_eq!(module.get_value_at("output", 0, 3), 5.0);
    // Slot 60 sits beyond the ceiling: still the buffer's unrendered zero.
    assert_eq!(module.get_value_at("output", 0, 60), 0.0);

    set_block_render_ceiling(usize::MAX);
    assert_eq!(module.get_value_at("output", 0, 60), 5.0);
}

/// Rendering a block in ceiling-bounded chunks must be sample-identical to
/// rendering it greedily — clamped rendering resumes from the same state.
#[test]
fn chunked_rendering_matches_greedy_rendering() {
    let chunked = make_module("$saw", "saw-chunked", json!({ "freq": 0.0 }));
    let greedy = make_module("$saw", "saw-greedy", json!({ "freq": 0.0 }));

    for _ in 0..3 {
        chunked.start_block();
        greedy.start_block();

        set_block_render_ceiling(usize::MAX);
        greedy.ensure_processed_to(BLOCK_SIZE);

        // Drive the chunked module the way the callback drains a split
        // block: reads clamped to successive emit boundaries.
        for ceiling in [9, 17, 40, BLOCK_SIZE] {
            set_block_render_ceiling(ceiling);
            let _ = chunked.get_value_at("output", 0, ceiling - 1);
        }

        set_block_render_ceiling(usize::MAX);
        for i in 0..BLOCK_SIZE {
            assert_eq!(
                chunked.get_value_at("output", 0, i),
                greedy.get_value_at("output", 0, i),
                "slot {i} diverged",
            );
        }
    }
}
