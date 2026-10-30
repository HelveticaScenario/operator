//! `$clamp` holds its input within the connected bounds, identically on the block
//! and per-sample paths.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn clamp_every_bound_combination() {
    let input = poly_cable("src", CHANNELS);
    check_module(
        "$clamp",
        json!({ "input": input.clone(), "min": -0.5, "max": cable("lfo", 0) }),
        |src, lfo| {
            let lfo0: Vec<f32> = (0..lfo.len())
                .map(|k| lfo[k / CHANNELS * CHANNELS])
                .collect();
            elementwise(src, &lfo0, |x, b| {
                let a = -0.5;
                let (lo, hi) = if b < a { (b, a) } else { (a, b) };
                x.clamp(lo, hi)
            })
        },
    );
    check_module(
        "$clamp",
        json!({ "input": input.clone(), "min": 0.2 }),
        |src, lfo| elementwise(src, lfo, |x, _| if x < 0.2 { 0.2 } else { x }),
    );
    check_module(
        "$clamp",
        json!({ "input": input.clone(), "max": poly_cable("lfo", CHANNELS) }),
        |src, lfo| elementwise(src, lfo, |x, max| if x > max { max } else { x }),
    );
    check_module("$clamp", json!({ "input": input }), |src, lfo| {
        elementwise(src, lfo, |x, _| x)
    });
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$clamp",
            json!({ "input": poly_cable("src", CHANNELS), "min": -1.0, "max": cable("src", 0) })
        ),
        0
    );
}
