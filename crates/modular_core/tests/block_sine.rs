//! `$sine` renders identically on the block and per-sample paths: held and
//! modulated pitch, FM, phase offset and hard sync.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn sine_paths_agree() {
    check_paths_agree("$sine", json!({ "freq": [0.0, 0.5, -1.0, 2.0] }));
    check_paths_agree(
        "$sine",
        json!({
            "freq": poly_cable("lfo", CHANNELS),
            "fm": cable("src", 1),
            "fmMode": "exp",
            "phaseOffset": 0.25,
        }),
    );
    check_paths_agree(
        "$sine",
        json!({ "freq": 1.0, "sync": poly_cable("src", CHANNELS), "phaseOffset": cable("lfo", 0) }),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$sine",
            json!({ "freq": poly_cable("src", CHANNELS), "fm": cable("src", 0), "sync": cable("src", 1) })
        ),
        0
    );
}
