use super::*;
use crate::patch::Patch;
use crate::types::{Connect, Signal};

fn constant(v: f32) -> PolySignal {
    PolySignal::mono(Signal::Volts(v))
}

// ---------- evaluate() ----------

#[test]
fn identity_evaluates_as_passthrough() {
    let t = PhaseWarp::Identity;
    for i in 0..=10 {
        let x = i as f32 / 10.0;
        assert!((t.evaluate(x, 0) - x).abs() < 1e-6);
    }
}

#[test]
fn mirror_evaluates_with_constant_signal() {
    let t = PhaseWarp::Mirror {
        amount: constant(0.0),
    };
    // amount=0 => identity.
    for i in 0..=10 {
        let x = i as f32 / 10.0;
        assert!((t.evaluate(x, 0) - x).abs() < 1e-6);
    }
    // amount=1 at midpoint => 1.0.
    let t = PhaseWarp::Mirror {
        amount: constant(1.0),
    };
    assert!((t.evaluate(0.5, 0) - 1.0).abs() < 1e-6);
}

#[test]
fn bend_evaluates_with_constant_signal() {
    let t = PhaseWarp::Bend {
        amount: constant(0.0),
    };
    for i in 0..=10 {
        let x = i as f32 / 10.0;
        assert!((t.evaluate(x, 0) - x).abs() < 1e-6);
    }
    // Endpoints preserved even under bend.
    let t = PhaseWarp::Bend {
        amount: constant(0.7),
    };
    assert!(t.evaluate(0.0, 0).abs() < 1e-6);
    assert!((t.evaluate(1.0, 0) - 1.0).abs() < 1e-6);
}

#[test]
fn sync_evaluates_with_constant_signal() {
    // ratio=0 => 1x (identity on [0, 1)).
    let t = PhaseWarp::Sync {
        ratio: constant(0.0),
    };
    for i in 0..100 {
        let x = i as f32 / 100.0;
        assert!((t.evaluate(x, 0) - x).abs() < 1e-6);
    }
}

#[test]
fn fold_evaluates_with_constant_signal() {
    // amount=0 => identity.
    let t = PhaseWarp::Fold {
        amount: constant(0.0),
    };
    for i in 0..=10 {
        let x = i as f32 / 10.0;
        assert!((t.evaluate(x, 0) - x).abs() < 1e-6);
    }
}

#[test]
fn pwm_evaluates_with_constant_signal() {
    let t = PhaseWarp::Pwm {
        width: constant(0.5),
    };
    assert!(t.evaluate(0.0, 0).abs() < 1e-6);
    assert!((t.evaluate(1.0, 0) - 1.0).abs() < 1e-6);
}

#[test]
fn evaluate_output_in_range_across_sweep() {
    let cases: Vec<PhaseWarp> = vec![
        PhaseWarp::Identity,
        PhaseWarp::Mirror {
            amount: constant(0.5),
        },
        PhaseWarp::Bend {
            amount: constant(0.5),
        },
        PhaseWarp::Sync {
            ratio: constant(0.5),
        },
        PhaseWarp::Fold {
            amount: constant(0.5),
        },
        PhaseWarp::Pwm {
            width: constant(0.3),
        },
    ];
    for t in &cases {
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            let y = t.evaluate(x, 0);
            assert!(
                (0.0..=1.0).contains(&y),
                "warp output {} out of range for x={}",
                y,
                x
            );
        }
    }
}

// ---------- connect() ----------

#[test]
fn connect_identity_is_noop() {
    let patch = Patch::new();
    let mut t = PhaseWarp::Identity;
    t.connect(&patch);
    assert!(matches!(t, PhaseWarp::Identity));
}

#[test]
fn connect_resolves_polysignal_for_every_variant() {
    // With PolySignal::mono(Signal::Volts(x)), `connect()` is a no-op on Volts,
    // so we mostly assert that it runs without panic and evaluates sensibly.
    let patch = Patch::new();

    let mut mirror = PhaseWarp::Mirror {
        amount: constant(0.25),
    };
    mirror.connect(&patch);
    // At x=0.5 the reflection is 1.0; interpolated at amount=0.25: 0.5 + 0.5*0.25 = 0.625.
    assert!((mirror.evaluate(0.5, 0) - 0.625).abs() < 1e-4);

    let mut bend = PhaseWarp::Bend {
        amount: constant(0.0),
    };
    bend.connect(&patch);
    assert!((bend.evaluate(0.5, 0) - 0.5).abs() < 1e-6);

    let mut sync = PhaseWarp::Sync {
        ratio: constant(0.0),
    };
    sync.connect(&patch);
    assert!((sync.evaluate(0.25, 0) - 0.25).abs() < 1e-6);

    let mut fold = PhaseWarp::Fold {
        amount: constant(0.0),
    };
    fold.connect(&patch);
    assert!((fold.evaluate(0.5, 0) - 0.5).abs() < 1e-6);

    let mut pwm = PhaseWarp::Pwm {
        width: constant(0.5),
    };
    pwm.connect(&patch);
    assert!((pwm.evaluate(0.5, 0) - 0.5).abs() < 1e-6);
}

// ---------- (de)serialization round trip ----------

#[test]
fn warp_deserializes_identity() {
    let t: PhaseWarp = serde_json::from_str(r#"{"type":"identity"}"#).unwrap();
    assert!(matches!(t, PhaseWarp::Identity));
}

#[test]
fn warp_deserializes_mirror_with_scalar_signal() {
    let t: PhaseWarp = serde_json::from_str(r#"{"type":"mirror","amount":0.5}"#).unwrap();
    match t {
        PhaseWarp::Mirror { amount } => {
            assert!((amount.get_value(0) - 0.5).abs() < 1e-6);
        }
        _ => panic!("expected Mirror"),
    }
}

#[test]
fn warp_deserializes_pwm_with_scalar_signal() {
    let t: PhaseWarp = serde_json::from_str(r#"{"type":"pwm","width":0.25}"#).unwrap();
    match t {
        PhaseWarp::Pwm { width } => {
            assert!((width.get_value(0) - 0.25).abs() < 1e-6);
        }
        _ => panic!("expected Pwm"),
    }
}

#[test]
fn warp_roundtrips_identity() {
    let t = PhaseWarp::Identity;
    let json = serde_json::to_string(&t).unwrap();
    assert_eq!(json, r#"{"type":"identity"}"#);
}

#[test]
fn warp_roundtrips_sync() {
    let t = PhaseWarp::Sync {
        ratio: constant(0.75),
    };
    let json = serde_json::to_string(&t).unwrap();
    let back: PhaseWarp = serde_json::from_str(&json).unwrap();
    match back {
        PhaseWarp::Sync { ratio } => {
            assert!((ratio.get_value(0) - 0.75).abs() < 1e-6);
        }
        _ => panic!("expected Sync"),
    }
}

// ---------- deserr parity ----------

#[test]
fn warp_deserr_parses_mirror() {
    use deserr::deserialize;

    let value: serde_json::Value =
        serde_json::from_str(r#"{"type":"mirror","amount":0.5}"#).unwrap();
    let t: PhaseWarp = deserialize::<PhaseWarp, _, deserr::errors::JsonError>(value).unwrap();
    match t {
        PhaseWarp::Mirror { amount } => {
            assert!((amount.get_value(0) - 0.5).abs() < 1e-6);
        }
        _ => panic!("expected Mirror"),
    }
}

#[test]
fn warp_deserr_parses_identity() {
    use deserr::deserialize;
    let value: serde_json::Value = serde_json::from_str(r#"{"type":"identity"}"#).unwrap();
    let t: PhaseWarp = deserialize::<PhaseWarp, _, deserr::errors::JsonError>(value).unwrap();
    assert!(matches!(t, PhaseWarp::Identity));
}

#[test]
fn warp_deserr_rejects_unknown_type() {
    use deserr::deserialize;
    let value: serde_json::Value = serde_json::from_str(r#"{"type":"bogus"}"#).unwrap();
    let res: StdResult<PhaseWarp, deserr::errors::JsonError> = deserialize(value);
    assert!(res.is_err());
}

// ---------- Pipe composition ----------

#[test]
fn pipe_chains_evaluate_first_then_second() {
    // mirror(0.5) at x=0.5 → reflected to 1.0 (full mirror).
    // Then bend(0.0) at 1.0 → 1.0 (identity at amount=0).
    let t = PhaseWarp::Pipe {
        first: Box::new(PhaseWarp::Mirror {
            amount: constant(1.0),
        }),
        second: Box::new(PhaseWarp::Bend {
            amount: constant(0.0),
        }),
    };
    // At x=0.5, mirror with amount=1 maps 0.5→1.0; bend with 0 maps 1.0→1.0.
    let out = t.evaluate(0.5, 0);
    assert!((out - 1.0).abs() < 1e-4, "expected ~1.0 got {out}");
}

#[test]
fn pipe_channels_is_max_of_both() {
    let t = PhaseWarp::Pipe {
        first: Box::new(PhaseWarp::Mirror {
            amount: constant(0.5),
        }),
        second: Box::new(PhaseWarp::Identity),
    };
    assert_eq!(t.channels(), 1); // Mirror has 1, Identity has 0
}

#[test]
fn pipe_serializes_and_deserializes_via_serde() {
    let t = PhaseWarp::Pipe {
        first: Box::new(PhaseWarp::Mirror {
            amount: constant(0.5),
        }),
        second: Box::new(PhaseWarp::Bend {
            amount: constant(0.3),
        }),
    };
    let json = serde_json::to_string(&t).unwrap();
    assert!(
        json.contains("\"type\":\"pipe\""),
        "missing pipe tag: {json}"
    );
    assert!(
        json.contains("\"type\":\"mirror\""),
        "missing first: {json}"
    );
    assert!(json.contains("\"type\":\"bend\""), "missing second: {json}");
    let back: PhaseWarp = serde_json::from_str(&json).unwrap();
    match back {
        PhaseWarp::Pipe { first, second } => {
            assert!(matches!(*first, PhaseWarp::Mirror { .. }));
            assert!(matches!(*second, PhaseWarp::Bend { .. }));
        }
        _ => panic!("expected Pipe"),
    }
}

#[test]
fn pipe_parses_via_deserr() {
    use deserr::deserialize;
    let value: serde_json::Value = serde_json::from_str(
        r#"{"type":"pipe","first":{"type":"mirror","amount":0.5},"second":{"type":"fold","amount":0.2}}"#,
    ).unwrap();
    let t: PhaseWarp = deserialize::<PhaseWarp, _, deserr::errors::JsonError>(value).unwrap();
    match t {
        PhaseWarp::Pipe { first, second } => {
            assert!(matches!(*first, PhaseWarp::Mirror { .. }));
            assert!(matches!(*second, PhaseWarp::Fold { .. }));
        }
        _ => panic!("expected Pipe"),
    }
}
