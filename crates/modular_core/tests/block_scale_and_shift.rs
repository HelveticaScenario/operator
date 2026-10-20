//! `$scaleAndShift` through every block-input resolution path: rows read in
//! place, constants and mono cables broadcast, inputs copied out slot by slot,
//! and the per-sample `update` path, pinned to its formula.

mod block_common;

use block_common::*;
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn expected(input: &[f32], scale: &[f32], shift: &[f32]) -> Vec<f32> {
    input
        .iter()
        .zip(scale)
        .zip(shift)
        .map(|((x, scale), shift)| x * (scale / 5.0) + shift)
        .collect()
}

/// Every sample of a `$scaleAndShift` with `params`; see [`check_module`].
fn check(params: Value, reference: impl Fn(&[f32], &[f32]) -> Vec<f32>) {
    check_module("$scaleAndShift", params, reference);
}

#[test]
fn poly_cable_with_constant_scale_and_shift() {
    check(
        json!({ "input": poly_cable("src", CHANNELS), "scale": 4.0, "shift": 0.5 }),
        |src, _| expected(src, &constant(4.0), &constant(0.5)),
    );
}

#[test]
fn disconnected_scale_and_shift_use_defaults() {
    check(json!({ "input": poly_cable("src", CHANNELS) }), |src, _| {
        expected(src, &constant(5.0), &constant(0.0))
    });
}

#[test]
fn every_input_a_cable() {
    check(
        json!({
            "input": poly_cable("src", CHANNELS),
            "scale": poly_cable("lfo", CHANNELS),
            "shift": poly_cable("lfo", CHANNELS),
        }),
        |src, lfo| expected(src, lfo, lfo),
    );
}

#[test]
fn constants_cycle_across_wider_channels() {
    // Two scale values cycle over four channels.
    check(
        json!({ "input": poly_cable("src", CHANNELS), "scale": [1.0, 3.0], "shift": [0.1, 0.2, 0.3, 0.4] }),
        |src, _| {
            let scale: Vec<f32> = (0..src.len())
                .map(|k| [1.0, 3.0][k % CHANNELS % 2])
                .collect();
            let shift: Vec<f32> = (0..src.len())
                .map(|k| [0.1, 0.2, 0.3, 0.4][k % CHANNELS])
                .collect();
            expected(src, &scale, &shift)
        },
    );
}

#[test]
fn mono_cable_broadcast_across_poly_channels() {
    // The four-wide shift makes the module four channels wide; the input is
    // channel 2 of `src` read on every channel.
    check(
        json!({ "input": cable("src", 2), "shift": [0.0, 1.0, 2.0, 3.0] }),
        |src, _| {
            let input: Vec<f32> = (0..src.len())
                .map(|k| src[k / CHANNELS * CHANNELS + 2])
                .collect();
            let shift: Vec<f32> = (0..src.len()).map(|k| (k % CHANNELS) as f32).collect();
            expected(&input, &constant(5.0), &shift)
        },
    );
}

#[test]
fn mixed_cable_and_constant_channels() {
    check(
        json!({
            "input": poly_cable("src", CHANNELS),
            "scale": [cable("lfo", 0), 2.0, cable("lfo", 2), 7.0],
        }),
        |src, lfo| {
            let scale: Vec<f32> = (0..src.len())
                .map(|k| match k % CHANNELS {
                    1 => 2.0,
                    3 => 7.0,
                    _ => lfo[k],
                })
                .collect();
            expected(src, &scale, &constant(0.0))
        },
    );
}

#[test]
fn permuted_cable_channels() {
    let reversed = Value::Array((0..CHANNELS).rev().map(|ch| cable("src", ch)).collect());
    check(json!({ "input": reversed, "scale": 2.5 }), |src, _| {
        let input: Vec<f32> = (0..src.len())
            .map(|k| src[k / CHANNELS * CHANNELS + (CHANNELS - 1 - k % CHANNELS)])
            .collect();
        expected(&input, &constant(2.5), &constant(0.0))
    });
}

#[test]
fn non_finite_results_are_silenced() {
    check(
        json!({ "input": poly_cable("src", CHANNELS), "scale": f32::MAX, "shift": f32::MAX }),
        |src, _| {
            expected(src, &constant(f32::MAX), &constant(f32::MAX))
                .into_iter()
                .map(|v| if v.is_finite() { v } else { 0.0 })
                .collect()
        },
    );
}

#[test]
fn mono_cable_modulates_poly_input() {
    // `.amp(envelope)`: a poly input scaled by one channel of another module.
    check(
        json!({ "input": poly_cable("src", CHANNELS), "scale": cable("lfo", 1) }),
        |src, lfo| {
            let scale: Vec<f32> = (0..src.len())
                .map(|k| lfo[k / CHANNELS * CHANNELS + 1])
                .collect();
            expected(src, &scale, &constant(0.0))
        },
    );
}

#[test]
fn cable_channels_starting_past_zero() {
    // Channels 1..4 of `src` make a three-wide module; reading its channel 3
    // wraps to channel 0, which is `src` channel 1.
    let shifted = Value::Array((1..CHANNELS).map(|ch| cable("src", ch)).collect());
    check(
        json!({ "input": shifted.clone(), "scale": 3.0 }),
        |src, _| {
            let input: Vec<f32> = (0..src.len())
                .map(|k| src[k / CHANNELS * CHANNELS + 1 + (k % CHANNELS) % (CHANNELS - 1)])
                .collect();
            expected(&input, &constant(3.0), &constant(0.0))
        },
    );
}

#[test]
fn paths_agree() {
    check_paths_agree(
        "$scaleAndShift",
        json!({ "input": poly_cable("src", CHANNELS), "scale": cable("lfo", 1), "shift": 0.3 }),
    );
}

#[test]
fn allocation_free() {
    for params in [
        json!({ "input": poly_cable("src", CHANNELS), "scale": 4.0, "shift": [0.1, 0.2, 0.3, 0.4] }),
        json!({ "input": poly_cable("src", CHANNELS) }),
        json!({ "input": poly_cable("src", CHANNELS), "scale": cable("src", 2) }),
        json!({ "input": cable("src", 1), "scale": [cable("src", 0), 2.0, 3.0, 4.0] }),
    ] {
        assert_eq!(allocations_while_rendering("$scaleAndShift", params), 0);
    }
}
