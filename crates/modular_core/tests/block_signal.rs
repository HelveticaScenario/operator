//! `$signal` forwards its source identically on the block and per-sample paths.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn signal_forwards_cables_and_constants() {
    check_module(
        "$signal",
        json!({ "source": poly_cable("src", CHANNELS) }),
        |src, lfo| elementwise(src, lfo, |x, _| x),
    );
    check_module(
        "$signal",
        json!({ "source": [0.5, -1.0, 2.0, 3.5] }),
        |src, _| {
            (0..src.len())
                .map(|k| [0.5, -1.0, 2.0, 3.5][k % CHANNELS])
                .collect()
        },
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering("$signal", json!({ "source": poly_cable("src", CHANNELS) })),
        0
    );
}
