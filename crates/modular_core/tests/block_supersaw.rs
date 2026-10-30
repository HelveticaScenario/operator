//! `$supersaw` renders identically on the block and per-sample paths: held and
//! modulated pitch and detune, FM, hard sync, and mono and poly inputs.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn supersaw_paths_agree() {
    check_paths_agree("$supersaw", json!({ "freq": 0.0, "voices": 4 }));
    check_paths_agree(
        "$supersaw",
        json!({ "freq": [0.0, 0.5, -1.0], "voices": 7, "detune": [0.3, 1.0, 0.0] }),
    );
    check_paths_agree(
        "$supersaw",
        json!({
            "freq": poly_cable("lfo", CHANNELS),
            "voices": 4,
            "fm": cable("src", 1),
            "fmMode": "exp",
            "detune": cable("lfo", 0),
        }),
    );
    check_paths_agree(
        "$supersaw",
        json!({ "freq": 1.0, "voices": 1, "sync": cable("src", 2), "waitForZeroCross": false }),
    );
    check_paths_agree(
        "$supersaw",
        json!({ "freq": [1.0, -1.0], "voices": 5, "sync": poly_cable("src", 2) }),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$supersaw",
            json!({ "freq": poly_cable("src", CHANNELS), "voices": 6, "fm": cable("src", 0), "sync": cable("src", 1) })
        ),
        0
    );
}
