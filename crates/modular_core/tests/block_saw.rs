//! `$saw` renders identically on the block and per-sample paths: held and
//! modulated pitch and shape, FM, phase offset and hard sync.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn saw_paths_agree() {
    check_paths_agree("$saw", json!({ "freq": [0.0, 0.5, -1.0, 2.0] }));
    check_paths_agree(
        "$saw",
        json!({
            "freq": poly_cable("lfo", CHANNELS),
            "fm": cable("src", 1),
            "fmMode": "exp",
            "shape": poly_cable("lfo", CHANNELS),
            "phaseOffset": 0.25,
        }),
    );
    check_paths_agree(
        "$saw",
        json!({
            "freq": -8.0,
            "shape": [0.0, 2.5, 5.0, 4.0],
            "waitForZeroCross": false,
        }),
    );
    check_paths_agree(
        "$saw",
        json!({ "freq": 1.0, "sync": poly_cable("src", CHANNELS), "phaseOffset": cable("lfo", 0) }),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$saw",
            json!({ "freq": poly_cable("src", CHANNELS), "fm": cable("src", 0), "sync": cable("src", 1) })
        ),
        0
    );
}
