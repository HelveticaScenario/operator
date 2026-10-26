//! `_arithmetic` and `_round` through every block-input resolution path and
//! the per-sample `update` path, pinned to their formulas.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

type BinaryOp = fn(f32, f32) -> f32;

const OPS: [(&str, BinaryOp); 5] = [
    ("add", |x, y| x + y),
    ("sub", |x, y| x - y),
    ("mul", |x, y| x * y),
    ("div", |x, y| x / y),
    ("pow", |x, y| x.signum() * x.abs().powf(y)),
];

const MODES: [(&str, fn(f32) -> f32); 3] = [
    ("round", f32::round),
    ("floor", f32::floor),
    ("ceil", f32::ceil),
];

fn silenced(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

#[test]
fn operations_with_a_cable_operand() {
    for (op, f) in OPS {
        check_module(
            "_arithmetic",
            json!({ "input": poly_cable("src", CHANNELS), "operand": poly_cable("lfo", CHANNELS), "op": op }),
            |src, lfo| {
                src.iter()
                    .zip(lfo)
                    .map(|(&x, &y)| silenced(f(x, y)))
                    .collect()
            },
        );
    }
}

#[test]
fn operations_with_constant_operands() {
    for (op, f) in OPS {
        check_module(
            "_arithmetic",
            json!({ "input": poly_cable("src", CHANNELS), "operand": [0.5, -2.0, 3.0, 0.25], "op": op }),
            |src, _| {
                src.iter()
                    .enumerate()
                    .map(|(k, &x)| silenced(f(x, [0.5, -2.0, 3.0, 0.25][k % CHANNELS])))
                    .collect()
            },
        );
    }
}

#[test]
fn division_by_zero_outputs_zero() {
    check_module(
        "_arithmetic",
        json!({ "input": poly_cable("src", CHANNELS), "operand": 0.0, "op": "div" }),
        |src, _| vec![0.0; src.len()],
    );
}

#[test]
fn op_defaults_to_add() {
    check_module(
        "_arithmetic",
        json!({ "input": poly_cable("src", CHANNELS), "operand": 1.5 }),
        |src, _| src.iter().map(|x| x + 1.5).collect(),
    );
}

#[test]
fn rounding_modes() {
    for (mode, f) in MODES {
        check_module(
            "_round",
            json!({ "input": poly_cable("src", CHANNELS), "mode": mode }),
            |src, _| src.iter().map(|&x| f(x)).collect(),
        );
    }
}

#[test]
fn paths_agree() {
    for (op, _) in OPS {
        check_paths_agree(
            "_arithmetic",
            json!({ "input": poly_cable("src", CHANNELS), "operand": cable("lfo", 1), "op": op }),
        );
    }
    for (mode, _) in MODES {
        check_paths_agree(
            "_round",
            json!({ "input": poly_cable("src", CHANNELS), "mode": mode }),
        );
    }
}

#[test]
fn allocation_free() {
    for (op, _) in OPS {
        let params = json!({ "input": poly_cable("src", CHANNELS), "operand": [cable("lfo", 0), 2.0], "op": op });
        assert_eq!(allocations_while_rendering("_arithmetic", params), 0);
    }
    for (mode, _) in MODES {
        let params = json!({ "input": poly_cable("src", CHANNELS), "mode": mode });
        assert_eq!(allocations_while_rendering("_round", params), 0);
    }
}
