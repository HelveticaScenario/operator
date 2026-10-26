//! `$stereoMix` renders identically on the block and per-sample paths: held
//! and modulated pan and width, and mono and poly inputs.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn stereo_mix_paths_agree() {
    check_paths_agree(
        "$stereoMix",
        json!({ "input": poly_cable("src", CHANNELS) }),
    );
    check_paths_agree(
        "$stereoMix",
        json!({ "input": poly_cable("src", CHANNELS), "pan": [-5.0, -1.0, 2.0, 5.0], "width": 2.5 }),
    );
    check_paths_agree(
        "$stereoMix",
        json!({
            "input": poly_cable("src", CHANNELS),
            "pan": poly_cable("lfo", CHANNELS),
            "width": poly_cable("lfo", 2),
        }),
    );
    check_paths_agree(
        "$stereoMix",
        json!({ "input": cable("src", 1), "pan": cable("lfo", 0) }),
    );
    // Eight channels gathered from two modules are copied, so a whole block
    // exceeds one scratch and resolves in shorter ranges.
    let wide: Vec<_> = (0..8)
        .map(|ch| cable(if ch % 2 == 0 { "src" } else { "lfo" }, ch % CHANNELS))
        .collect();
    check_paths_agree(
        "$stereoMix",
        json!({ "input": wide, "pan": poly_cable("lfo", CHANNELS), "width": 1.5 }),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$stereoMix",
            json!({ "input": poly_cable("src", CHANNELS), "pan": cable("lfo", 0), "width": cable("lfo", 1) })
        ),
        0
    );
}
