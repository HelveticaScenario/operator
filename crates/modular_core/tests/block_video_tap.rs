//! `_videoTap` passes its input through identically on the block and
//! per-sample paths, for one cable and for several summed.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn video_tap_paths_agree() {
    check_paths_agree("_videoTap", json!({ "input": cable("src", 1), "slot": 40 }));
    check_paths_agree(
        "_videoTap",
        json!({ "input": poly_cable("src", CHANNELS), "slot": 41 }),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "_videoTap",
            json!({ "input": poly_cable("lfo", CHANNELS), "slot": 42 })
        ),
        0
    );
}
