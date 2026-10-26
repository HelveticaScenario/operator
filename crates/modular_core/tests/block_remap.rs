//! `$remap` smooths its range and maps its input identically on the block and
//! per-sample paths.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn remap_smooths_its_range_per_channel() {
    use modular_core::types::Clickless;
    check_module(
        "$remap",
        json!({
            "input": poly_cable("src", CHANNELS),
            "inMin": -5.0,
            "inMax": 5.0,
            "outMin": [-1.0, -2.0, 0.0, 1.0],
            "outMax": cable("lfo", 2),
        }),
        |src, lfo| {
            let mut states = [[Clickless::default(); 4]; CHANNELS];
            src.iter()
                .enumerate()
                .map(|(k, &x)| {
                    let ch = k % CHANNELS;
                    let range = [-5.0, 5.0, [-1.0, -2.0, 0.0, 1.0][ch], lfo[k - ch + 2]];
                    for (state, target) in states[ch].iter_mut().zip(range) {
                        state.update(target);
                    }
                    let [in_min, in_max, out_min, out_max] = states[ch].map(|s| *s);
                    let denom = in_max - in_min;
                    let v = if denom.abs() < f32::EPSILON {
                        out_min
                    } else {
                        (x - in_min) * (out_max - out_min) / denom + out_min
                    };
                    if v.is_finite() { v } else { 0.0 }
                })
                .collect()
        },
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$remap",
            json!({ "input": poly_cable("src", CHANNELS), "inMin": -5.0, "inMax": 5.0, "outMin": -1.0, "outMax": cable("src", 3) })
        ),
        0
    );
}
