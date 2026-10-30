//! The `$mi.*` oscillators render identically on the block and per-sample
//! paths: held and modulated controls, audio-rate and through-zero FM, mono and
//! poly inputs, with control ticks falling inside and across render steps.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn formant_paths_agree() {
    check_paths_agree("$mi.formant", json!({ "freq": 0.0 }));
    check_paths_agree(
        "$mi.formant",
        json!({
            "freq": [-1.0, 0.0, 1.5, 3.0],
            "formant": cable("lfo", 0),
            "ratio": poly_cable("lfo", CHANNELS),
            "width": 4.0,
            "fm": cable("src", 1),
        }),
    );
    check_paths_agree(
        "$mi.formant",
        json!({
            "freq": cable("lfo", 2),
            "ratio": 1.0,
            "fm": poly_cable("src", 3),
            "fmMode": "exp",
        }),
    );
}

#[test]
fn z_osc_paths_agree() {
    check_paths_agree("$mi.zOsc", json!({ "freq": 0.0 }));
    check_paths_agree(
        "$mi.zOsc",
        json!({
            "freq": [-1.0, 0.0, 1.5, 3.0],
            "cutoff": cable("lfo", 0),
            "shape": poly_cable("lfo", CHANNELS),
            "mode": 1.0,
            "fm": cable("src", 1),
        }),
    );
    check_paths_agree(
        "$mi.zOsc",
        json!({
            "freq": cable("lfo", 2),
            "shape": 4.5,
            "mode": 4.0,
            "fm": poly_cable("src", 3),
            "fmMode": "lin",
        }),
    );
}

#[test]
fn allocation_free() {
    for module in ["$mi.formant", "$mi.zOsc"] {
        assert_eq!(
            allocations_while_rendering(
                module,
                json!({ "freq": poly_cable("lfo", CHANNELS), "fm": cable("src", 0) })
            ),
            0,
            "{module}"
        );
    }
}
