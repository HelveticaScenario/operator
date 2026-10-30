use super::*;
use crate::patch::Patch;
use crate::types::{Connect, Signal};

const SAMPLE_RATE: f32 = 48000.0;

/// Warp `x` on channel 0, resolving the stage values with `glides`.
fn warp_with(
    t: &PhaseWarp,
    x: f32,
    freq_norm: f32,
    glides: &mut PhaseWarpGlides,
    state: &mut PhaseWarpState,
) -> f32 {
    let resolved = t.resolve(0, SAMPLE_RATE, glides);
    t.evaluate(x, &resolved, freq_norm, state)
}

fn ev(t: &PhaseWarp, x: f32) -> f32 {
    warp_with(
        t,
        x,
        0.0,
        &mut PhaseWarpGlides::default(),
        &mut PhaseWarpState::default(),
    )
}

fn constant(v: f32) -> PolySignal {
    PolySignal::mono(Signal::Volts(v))
}

// ---------- evaluate() ----------

#[test]
fn identity_evaluates_as_passthrough() {
    let t = PhaseWarp::Identity;
    for i in 0..=10 {
        let x = i as f32 / 10.0;
        assert!((ev(&t, x) - x).abs() < 1e-6);
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
        assert!((ev(&t, x) - x).abs() < 1e-6);
    }
    // amount=5 (full) at midpoint => 1.0.
    let t = PhaseWarp::Mirror {
        amount: constant(5.0),
    };
    assert!((ev(&t, 0.5) - 1.0).abs() < 1e-6);
}

#[test]
fn bend_evaluates_with_constant_signal() {
    let t = PhaseWarp::Bend {
        amount: constant(0.0),
    };
    for i in 0..=10 {
        let x = i as f32 / 10.0;
        assert!((ev(&t, x) - x).abs() < 1e-6);
    }
    // Endpoints preserved even under bend.
    let t = PhaseWarp::Bend {
        amount: constant(3.5),
    };
    assert!(ev(&t, 0.0).abs() < 1e-6);
    assert!((ev(&t, 1.0) - 1.0).abs() < 1e-6);
}

#[test]
fn sync_evaluates_with_constant_signal() {
    // ratio=0 => 1x (identity on [0, 1)).
    let t = PhaseWarp::Sync {
        ratio: constant(0.0),
    };
    for i in 0..100 {
        let x = i as f32 / 100.0;
        assert!((ev(&t, x) - x).abs() < 1e-6);
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
        assert!((ev(&t, x) - x).abs() < 1e-6);
    }
}

#[test]
fn pwm_evaluates_with_constant_signal() {
    let t = PhaseWarp::Pwm {
        width: constant(2.5),
    };
    assert!(ev(&t, 0.0).abs() < 1e-6);
    assert!((ev(&t, 1.0) - 1.0).abs() < 1e-6);
}

#[test]
fn evaluate_output_in_range_across_sweep() {
    let cases: Vec<PhaseWarp> = vec![
        PhaseWarp::Identity,
        PhaseWarp::Mirror {
            amount: constant(2.5),
        },
        PhaseWarp::Bend {
            amount: constant(2.5),
        },
        PhaseWarp::Sync {
            ratio: constant(2.5),
        },
        PhaseWarp::Fold {
            amount: constant(2.5),
        },
        PhaseWarp::Pwm {
            width: constant(1.5),
        },
    ];
    for t in &cases {
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            let y = ev(t, x);
            assert!(
                (0.0..=1.0).contains(&y),
                "table variant output {} out of range for x={}",
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
        amount: constant(1.25),
    };
    mirror.connect(&patch);
    // At x=0.5 the reflection is 1.0; a quarter mirror (1.25 of 5) gives 0.625.
    assert!((ev(&mirror, 0.5) - 0.625).abs() < 1e-4);

    let mut bend = PhaseWarp::Bend {
        amount: constant(0.0),
    };
    bend.connect(&patch);
    assert!((ev(&bend, 0.5) - 0.5).abs() < 1e-6);

    let mut sync = PhaseWarp::Sync {
        ratio: constant(0.0),
    };
    sync.connect(&patch);
    assert!((ev(&sync, 0.25) - 0.25).abs() < 1e-6);

    let mut fold = PhaseWarp::Fold {
        amount: constant(0.0),
    };
    fold.connect(&patch);
    assert!((ev(&fold, 0.5) - 0.5).abs() < 1e-6);

    let mut pwm = PhaseWarp::Pwm {
        width: constant(2.5),
    };
    pwm.connect(&patch);
    assert!((ev(&pwm, 0.5) - 0.5).abs() < 1e-6);
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
    // A full mirror maps x=0.5 to 1.0; bend at 0 is the identity.
    let t = PhaseWarp::Pipe {
        first: Box::new(PhaseWarp::Mirror {
            amount: constant(5.0),
        }),
        second: Box::new(PhaseWarp::Bend {
            amount: constant(0.0),
        }),
    };
    let out = ev(&t, 0.5);
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

// ---------- Ensemble Oscillator twists ----------

#[test]
fn twists_are_identity_at_zero_amount() {
    for warp in [
        PhaseWarp::Pulsar {
            amount: constant(0.0),
        },
        PhaseWarp::Crush {
            amount: constant(0.0),
        },
    ] {
        for i in 0..100 {
            let x = i as f32 / 100.0;
            assert!((ev(&warp, x) - x).abs() < 1e-6, "{warp:?} at {x}");
        }
    }
}

#[test]
fn pulsar_compresses_then_rests_at_zero() {
    // amount 5 → 64×: the cycle completes in the first 1/64 of the phase.
    let warp = PhaseWarp::Pulsar {
        amount: constant(5.0),
    };
    assert!((ev(&warp, 0.5 / 64.0) - 0.5).abs() < 1e-4);
    assert_eq!(ev(&warp, 0.5), 0.0);
}

#[test]
fn pulsar_antialiasing_backs_off_near_nyquist() {
    let warp = PhaseWarp::Pulsar {
        amount: constant(5.0),
    };
    let mut state = PhaseWarpState::default();
    // At half the sample rate the rolloff is total: the warp is the identity.
    assert!(
        (warp_with(&warp, 0.3, 0.5, &mut PhaseWarpGlides::default(), &mut state) - 0.3).abs()
            < 1e-6
    );
}

#[test]
fn feedback_offsets_by_the_lowpassed_sine() {
    // Reference: phase += lp · a + a, wrapped, with a = 0.7 · k².
    let warp = PhaseWarp::Feedback {
        amount: constant(5.0),
    };
    let mut state = PhaseWarpState::default();
    // Empty lowpass: only the constant offset.
    let y = warp_with(&warp, 0.1, 0.0, &mut PhaseWarpGlides::default(), &mut state);
    assert!((y - 0.8).abs() < 1e-6, "got {y}");
    let lp = (y * std::f32::consts::TAU).sin() * 0.25;
    let expected = (0.1 + lp * 0.7 + 0.7_f32).fract();
    let y = warp_with(&warp, 0.1, 0.0, &mut PhaseWarpGlides::default(), &mut state);
    assert!((y - expected).abs() < 1e-6, "got {y}, expected {expected}");
}

#[test]
fn feedback_stages_keep_separate_state() {
    let fb = || {
        Box::new(PhaseWarp::Feedback {
            amount: constant(3.0),
        })
    };
    let piped = PhaseWarp::Pipe {
        first: fb(),
        second: fb(),
    };
    let mut state = PhaseWarpState::default();
    for _ in 0..10 {
        warp_with(
            &piped,
            0.2,
            0.0,
            &mut PhaseWarpGlides::default(),
            &mut state,
        );
    }
    assert!(state.feedback[0] != 0.0 && state.feedback[1] != 0.0);
    assert!(state.feedback[0] != state.feedback[1]);
}

#[test]
fn deserr_rejects_too_many_feedback_stages() {
    let fb = r#"{"type":"feedback","amount":1}"#;
    let mut json = fb.to_string();
    for _ in 0..MAX_FEEDBACK_STAGES {
        json = format!(r#"{{"type":"pipe","first":{fb},"second":{json}}}"#);
    }
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(deserr::deserialize::<PhaseWarp, _, deserr::errors::JsonError>(value).is_err());
}

#[test]
fn deserr_parses_twists() {
    for kind in ["pulsar", "crush", "feedback"] {
        let value: serde_json::Value =
            serde_json::from_str(&format!(r#"{{"type":"{kind}","amount":2}}"#)).unwrap();
        let warp = deserr::deserialize::<PhaseWarp, _, deserr::errors::JsonError>(value)
            .unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert_eq!(warp.channels(), 1);
    }
}

// ---------- glides across patch swaps ----------

/// Glide state that has run `old` for a while, and `new` marked against it as
/// a patch swap marks it.
fn swapped(old: &PhaseWarp, mut new: PhaseWarp) -> (PhaseWarp, PhaseWarpGlides) {
    let mut glides = PhaseWarpGlides::default();
    for _ in 0..100 {
        old.resolve(0, SAMPLE_RATE, &mut glides);
    }
    new.mark_glides(Some(old), 1, &crate::types::SameIds);
    (new, glides)
}

/// The value `new` reads on its first sample after replacing `old`.
fn first_value_after_swap(old: &PhaseWarp, new: PhaseWarp) -> f32 {
    let (new, mut glides) = swapped(old, new);
    new.resolve(0, SAMPLE_RATE, &mut glides).values[0]
}

#[test]
fn an_edited_amount_glides_from_the_old_value() {
    let old = PhaseWarp::Mirror {
        amount: constant(0.0),
    };
    let (new, mut glides) = swapped(
        &old,
        PhaseWarp::Mirror {
            amount: constant(5.0),
        },
    );
    // Resolved values are shape parameters: a mirror knob of 5 is 1.
    let first = new.resolve(0, SAMPLE_RATE, &mut glides).values[0];
    assert!(first < 0.1, "expected to start near 0, got {first}");
    let mut last = first;
    for _ in 0..(SAMPLE_RATE as usize) {
        last = new.resolve(0, SAMPLE_RATE, &mut glides).values[0];
    }
    assert!(
        (last - 1.0).abs() < 1e-3,
        "expected to settle on 1, got {last}"
    );
}

#[test]
fn an_unchanged_amount_does_not_glide() {
    let old = PhaseWarp::Pulsar {
        amount: constant(2.0),
    };
    let first = first_value_after_swap(&old, old.clone());
    assert_eq!(first, shapes::pulsar_amount(2.0));
}

#[test]
fn a_changed_stage_kind_starts_on_its_value() {
    let old = PhaseWarp::Pulsar {
        amount: constant(0.0),
    };
    let new = PhaseWarp::Crush {
        amount: constant(5.0),
    };
    assert_eq!(first_value_after_swap(&old, new), shapes::crush_amount(5.0));
}

#[test]
fn nested_stages_glide_independently() {
    let pipe = |a: f32, b: f32| PhaseWarp::Pipe {
        first: Box::new(PhaseWarp::Bend {
            amount: constant(a),
        }),
        second: Box::new(PhaseWarp::Fold {
            amount: constant(b),
        }),
    };
    let (new, mut glides) = swapped(&pipe(0.0, 2.5), pipe(5.0, 2.5));
    let resolved = new.resolve(0, SAMPLE_RATE, &mut glides);
    assert!(
        resolved.values[0] < 0.1,
        "edited stage glides: {}",
        resolved.values[0]
    );
    assert_eq!(resolved.values[1], 0.5, "unedited stage passes through");
}

#[test]
fn deserr_rejects_too_many_stages() {
    let stage = r#"{"type":"mirror","amount":0.5}"#;
    let mut json = stage.to_string();
    for _ in 0..MAX_SIGNAL_STAGES {
        json = format!(r#"{{"type":"pipe","first":{stage},"second":{json}}}"#);
    }
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(deserr::deserialize::<PhaseWarp, _, deserr::errors::JsonError>(value).is_err());
}
