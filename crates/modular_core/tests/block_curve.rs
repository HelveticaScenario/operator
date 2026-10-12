//! `$curve` bends its input identically on the block and per-sample paths.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn curve_shapes_by_exponent() {
    let curve = |x: f32, exp: f32| {
        let normalized = (x.abs() / 5.0).max(0.0);
        x.signum() * 5.0 * normalized.powf(exp.max(0.0))
    };
    check_module(
        "$curve",
        json!({ "input": poly_cable("src", CHANNELS), "exp": 2.5 }),
        |src, lfo| elementwise(src, lfo, |x, _| curve(x, 2.5)),
    );
    check_module(
        "$curve",
        json!({ "input": poly_cable("src", CHANNELS), "exp": poly_cable("lfo", CHANNELS) }),
        |src, lfo| elementwise(src, lfo, curve),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$curve",
            json!({ "input": poly_cable("src", CHANNELS), "exp": 2.0 })
        ),
        0
    );
}
