//! `$addHz` renders identically on the block and per-sample paths, with its
//! `offset` read as a cable, a constant, or per-channel constants.

mod block_common;

use block_common::*;
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn cases() -> [Value; 3] {
    [
        json!({ "input": poly_cable("src", CHANNELS), "offset": cable("lfo", 1) }),
        json!({ "input": poly_cable("src", CHANNELS), "offset": 1.5 }),
        json!({ "input": cable("src", 2), "offset": [0.5, 1.0, 2.0, 3.0] }),
    ]
}

#[test]
fn paths_agree() {
    for params in cases() {
        check_paths_agree("$addHz", params);
    }
}

#[test]
fn allocation_free() {
    for params in cases() {
        assert_eq!(allocations_while_rendering("$addHz", params), 0);
    }
}
