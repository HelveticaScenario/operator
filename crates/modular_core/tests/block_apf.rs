//! `$apf` renders identically on the block and per-sample paths: whole and
//! fractional stage counts, modulated stages crossing stage boundaries, resonance,
//! feedback, crossfaded pole counts, and mono and poly inputs.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn apf_paths_agree() {
    check_paths_agree("$apf", json!({ "input": cable("src", 0), "cutoff": 1.0 }));
    check_paths_agree(
        "$apf",
        json!({
            "input": poly_cable("src", CHANNELS),
            "cutoff": poly_cable("lfo", CHANNELS),
            "stages": [1.0, 1.25, 3.6, 50.0],
            "resonance": 3.0,
            "feedback": -4.0,
        }),
    );
    // Poles swept across odd and even counts by an LFO.
    check_paths_agree(
        "$apf",
        json!({
            "input": cable("src", 1),
            "cutoff": cable("lfo", 0),
            "stages": [cable("lfo", 1), 6.0, cable("lfo", 3)],
            "resonance": cable("lfo", 2),
            "feedback": [4.5, cable("lfo", 0)],
        }),
    );
    // Crossfading across grid points, odd and even.
    check_paths_agree(
        "$apf",
        json!({
            "input": poly_cable("src", CHANNELS),
            "cutoff": 0.5,
            "stages": [cable("lfo", 1), 13.0, 3.25, 49.5],
            "resonance": 2.0,
            "feedback": 3.0,
            "fadeStep": 2,
        }),
    );
    // Self-oscillating feedback, both polarities.
    check_paths_agree(
        "$apf",
        json!({
            "input": cable("src", 0),
            "cutoff": -1.0,
            "stages": [4.0, 4.75],
            "feedback": [6.0, -5.5],
        }),
    );
}

#[test]
fn allocation_free() {
    for params in [
        json!({
            "input": poly_cable("src", CHANNELS),
            "cutoff": cable("lfo", 0),
            "stages": [cable("lfo", 1), 49.75, 1.5, 25.0],
            "resonance": cable("lfo", 2),
            "feedback": 2.0,
        }),
        json!({
            "input": poly_cable("src", CHANNELS),
            "cutoff": cable("lfo", 0),
            "stages": cable("lfo", 1),
            "resonance": 4.0,
            "feedback": cable("lfo", 2),
        }),
        json!({
            "input": poly_cable("src", CHANNELS),
            "cutoff": cable("lfo", 0),
            "stages": [cable("lfo", 1), 20.0, 2.75, 50.0],
            "fadeStep": 8,
            "feedback": 5.8,
        }),
    ] {
        assert_eq!(allocations_while_rendering("$apf", params), 0);
    }
}
