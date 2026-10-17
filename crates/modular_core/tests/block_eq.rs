//! `$eq` renders identically on the block and per-sample paths: fixed sections
//! that settle on their first sample, swept sections that keep smoothing, a
//! freq summed from several channels, per-channel lists, and more smoothing
//! sections than one block range resolves.

mod block_common;

use block_common::*;
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn fixed() -> Value {
    json!([
        { "type": "lowShelf", "freq": -2.0, "gain": 3.0 },
        { "type": "peak", "freq": 1.0, "gain": -6.0, "q": 2.0 },
        { "type": "highpass", "freq": -3.0 },
    ])
}

fn swept() -> Value {
    json!([
        { "type": "highShelf", "freq": 2.0, "gain": cable("lfo", 1) },
        { "type": "peak", "freq": cable("lfo", 0), "gain": 9.0, "q": 4.0 },
        { "type": "notch", "freq": [cable("lfo", 2), cable("lfo", 3)], "q": cable("lfo", 1) },
        { "type": "lowpass", "freq": 3.0, "q": 0.5 },
    ])
}

#[test]
fn eq_paths_agree() {
    let input = poly_cable("src", CHANNELS);
    check_paths_agree("$eq", json!({ "input": input, "sections": fixed() }));
    check_paths_agree("$eq", json!({ "input": input, "sections": swept() }));
    check_paths_agree(
        "$eq",
        json!({ "input": input, "sections": swept(), "adaptiveQ": true }),
    );
    check_paths_agree("$eq", json!({ "input": cable("src", 2), "sections": [] }));
    check_paths_agree(
        "$eq",
        json!({ "input": input, "sections": [fixed(), swept()] }),
    );
    check_paths_agree(
        "$eq",
        json!({ "input": input, "sections": [swept(), swept(), swept(), fixed()] }),
    );
}

#[test]
fn allocation_free() {
    assert_eq!(
        allocations_while_rendering(
            "$eq",
            json!({ "input": poly_cable("src", CHANNELS), "sections": [fixed(), swept()] })
        ),
        0
    );
}
