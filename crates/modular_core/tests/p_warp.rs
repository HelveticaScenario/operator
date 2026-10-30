//! `$pWarp` renders without touching the heap, with warp signals as cables
//! and constants and feedback stages carrying state.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn allocation_free() {
    let params = json!({
        "input": poly_cable("src", CHANNELS),
        "phase": { "type": "pipe",
                   "first": { "type": "mirror", "amount": cable("lfo", 0) },
                   "second": { "type": "pipe",
                               "first": { "type": "feedback", "amount": 2.0 },
                               "second": { "type": "pulsar", "amount": poly_cable("lfo", CHANNELS) } } },
        "freq": 0.0,
    });
    assert_eq!(allocations_while_rendering("$pWarp", params), 0);
}
