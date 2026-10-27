//! `_hz` renders identically on the block and per-sample paths, pinned to
//! log2(f / C4) with frequencies at or below 0.0001 Hz floored there.

mod block_common;

use block_common::*;
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const C4_HZ: f32 = 261.625_58;

fn cases() -> [Value; 2] {
    [
        json!({ "input": poly_cable("src", CHANNELS) }),
        json!({ "input": cable("src", 2) }),
    ]
}

#[test]
fn converts_hz_to_voct() {
    check_module(
        "_hz",
        json!({ "input": poly_cable("src", CHANNELS) }),
        |src, _| src.iter().map(|&f| (f.max(1e-4) / C4_HZ).log2()).collect(),
    );
}

#[test]
fn paths_agree() {
    for params in cases() {
        check_paths_agree("_hz", params);
    }
}

#[test]
fn allocation_free() {
    for params in cases() {
        assert_eq!(allocations_while_rendering("_hz", params), 0);
    }
}
