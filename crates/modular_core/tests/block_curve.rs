//! `$curve` renders identically on the block and per-sample paths, with its
//! `exp` read as a cable, a constant, or per-channel constants.

mod block_common;

use block_common::*;
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn cases() -> [Value; 3] {
    [
        json!({ "input": poly_cable("src", CHANNELS), "exp": cable("lfo", 1) }),
        json!({ "input": poly_cable("src", CHANNELS), "exp": 1.5 }),
        json!({ "input": cable("src", 2), "exp": [0.5, 1.0, 2.0, 3.0] }),
    ]
}

#[test]
fn paths_agree() {
    for params in cases() {
        check_paths_agree("$curve", params);
    }
}

#[test]
fn allocation_free() {
    for params in cases() {
        assert_eq!(allocations_while_rendering("$curve", params), 0);
    }
}
