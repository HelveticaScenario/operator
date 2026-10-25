//! `$multimode` renders identically on the block and per-sample paths: held
//! and modulated controls, mono and poly inputs, a group with one live channel,
//! self-oscillation, and both resolutions.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn multimode_paths_agree() {
    check_paths_agree(
        "$multimode",
        json!({ "input": cable("src", 0), "cutoff": 1.0 }),
    );
    check_paths_agree(
        "$multimode",
        json!({
            "input": poly_cable("src", CHANNELS),
            "cutoff": poly_cable("lfo", CHANNELS),
            "resonance": 3.5,
            "mode": cable("lfo", 1),
            "drive": [0.0, 2.0, 4.0],
            "fold": cable("lfo", 2),
        }),
    );
    // Five channels: a full group, then a group with one live channel.
    check_paths_agree(
        "$multimode",
        json!({
            "input": cable("src", 1),
            "cutoff": [0.0, 1.0, 2.0, -1.0, 0.5],
            "resonance": [1.0, 4.5, 2.0, 3.0, 5.0],
            "mode": 2.5,
        }),
    );
    check_paths_agree(
        "$multimode",
        json!({ "input": 0.0, "cutoff": -1.0, "resonance": 5.0, "highRes": false }),
    );
    check_paths_agree(
        "$multimode",
        json!({
            "input": poly_cable("src", 2),
            "cutoff": cable("lfo", 0),
            "resonance": 4.0,
            "mode": 4.0,
            "drive": 3.0,
            "highRes": false,
        }),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$multimode",
            json!({
                "input": poly_cable("src", CHANNELS),
                "cutoff": cable("lfo", 0),
                "resonance": cable("lfo", 1),
                "mode": 2.0,
                "drive": poly_cable("lfo", 3),
                "fold": 1.0,
            })
        ),
        0
    );
}
