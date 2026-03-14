use std::collections::HashMap;
use std::sync::{Arc, Weak};

use napi::Result;
use serde::Deserialize;
use serde_json::json;

use modular_core::patch::Patch;
use modular_core::types::{
    ClockMessages, Connect, Message, MessageHandler, MessageTag, MidiControlChange, MidiNoteOn,
    Sampleable, Signal, SignalExt,
};

// The proc-macro expands to `crate::types::...`; provide that module in this integration test crate.
mod types {
    pub use modular_core::types::*;
}

#[derive(Default)]
struct DummySampleable {
    id: String,
    module_type: String,
    outputs: HashMap<String, f32>,
}

impl DummySampleable {
    fn new(
        id: &str,
        module_type: &str,
        outputs: impl IntoIterator<Item = (impl Into<String>, f32)>,
    ) -> Self {
        Self {
            id: id.to_string(),
            module_type: module_type.to_string(),
            outputs: outputs.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        }
    }
}

impl Sampleable for DummySampleable {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn tick(&self) {}

    fn update(&self) {}

    fn get_poly_sample(&self, port: &str) -> Result<modular_core::poly::PolyOutput> {
        Ok(modular_core::poly::PolyOutput::mono(
            *self.outputs.get(port).unwrap_or(&0.0),
        ))
    }

    fn get_module_type(&self) -> &str {
        &self.module_type
    }

    fn apply_deserialized_params(
        &self,
        _deserialized: modular_core::params::DeserializedParams,
    ) -> Result<()> {
        Ok(())
    }
    fn connect(&self, _patch: &Patch) {
        println!("Connecting DummySampleable {}", self.id);
    }
}

impl MessageHandler for DummySampleable {}

fn make_empty_patch() -> Patch {
    Patch::new()
}

fn make_patch_with_sampleable(sampleable: Arc<Box<dyn Sampleable>>) -> Patch {
    let mut patch = Patch::new();
    patch
        .sampleables
        .insert(sampleable.get_id().to_owned(), sampleable);

    patch
}

/*
fn make_patch_with_track(track: Arc<Track>) -> Patch {
    let mut tracks: TrackMap = HashMap::new();
    tracks.insert(track.id.clone(), track);
    Patch::new(HashMap::new(), tracks)
}
*/

fn approx_eq(a: f32, b: f32, eps: f32) {
    assert!(
        (a - b).abs() <= eps,
        "expected {a} ~ {b} (eps {eps}), diff {}",
        (a - b).abs()
    );
}

#[test]
fn signal_volts_get_value() {
    let s = Signal::Volts(-1.23);
    approx_eq(s.get_value(), -1.23, 1e-6);
}

#[test]
fn option_signal_none_value_or() {
    let s: Option<Signal> = None;
    approx_eq(s.value_or(42.0), 42.0, 1e-6);
    approx_eq(s.value_or_zero(), 0.0, 1e-6);
}

#[test]
fn option_signal_some_value_or() {
    let s: Option<Signal> = Some(Signal::Volts(3.5));
    approx_eq(s.value_or(42.0), 3.5, 1e-6);
    approx_eq(s.value_or_zero(), 3.5, 1e-6);
}

#[test]
fn signal_deserialize_number_as_volts() {
    let s: Signal = serde_json::from_value(json!(1.25)).unwrap();
    match s {
        Signal::Volts(v) => approx_eq(v, 1.25, 1e-6),
        other => panic!("expected Signal::Volts, got {other:?}"),
    }
}

#[test]
fn signal_deserialize_tagged_variants_still_work() {
    // Note: Volts are deserialized as bare numbers, not as tagged variants
    // So {"type":"volts","value":-2.0} is NOT supported - use -2.0 directly
    let volts: Signal = serde_json::from_value(json!(-2.0)).unwrap();
    assert!(matches!(volts, Signal::Volts(v) if (v + 2.0).abs() < 1e-6));

    let cable: Signal =
        serde_json::from_value(json!({"type":"cable","module":"m1","port":"out"})).unwrap();
    match cable {
        Signal::Cable {
            module,
            port,
            module_ptr,
            channel,
        } => {
            assert_eq!(module, "m1");
            assert_eq!(port, "out");
            assert_eq!(channel, 0);
            assert!(module_ptr.upgrade().is_none());
        }
        other => panic!("expected Signal::Cable, got {other:?}"),
    }

    /*
    let track: Signal = serde_json::from_value(json!({"type":"track","track":"t1"})).unwrap();
    match track {
        Signal::Track { track, track_ptr } => {
            assert_eq!(track, "t1");
            assert!(track_ptr.upgrade().is_none());
        }
        other => panic!("expected Signal::Track, got {other:?}"),
    }
    */

    // Signal::Disconnected has been removed — deserialization of {"type":"disconnected"}
    // should now fail.
    let disconnected_result: std::result::Result<Signal, _> =
        serde_json::from_value(json!({"type":"disconnected"}));
    assert!(
        disconnected_result.is_err(),
        "Signal::Disconnected was removed; deserialization should fail"
    );
}

#[test]
fn signal_cable_connect_and_read() {
    let sampleable: Arc<Box<dyn Sampleable>> = Arc::new(Box::new(DummySampleable::new(
        "m1",
        "dummy",
        [("out", 3.5)],
    )));
    let patch = make_patch_with_sampleable(Arc::clone(&sampleable));

    let mut s = Signal::Cable {
        module: "m1".to_string(),
        module_ptr: Weak::new(),
        port: "out".to_string(),
        channel: 0,
    };

    // Before connect, cable reads 0.0 (module_ptr doesn't resolve).
    approx_eq(s.get_value(), 0.0, 1e-6);

    s.connect(&patch);
    approx_eq(s.get_value(), 3.5, 1e-6);
}

/*
#[test]
fn signal_track_connect_and_read() {
    let track = Arc::new(Track::new(
        "t1".to_string(),
        Signal::Volts { value: -5.0 },
        InterpolationType::Linear,
    ));

    track.add_keyframe(TrackKeyframe {
        id: "k1".to_string(),
        track_id: "t1".to_string(),
        time: 0.0,
        signal: Signal::Volts { value: 2.0 },
    });
    track.add_keyframe(TrackKeyframe {
        id: "k2".to_string(),
        track_id: "t1".to_string(),
        time: 1.0,
        signal: Signal::Volts { value: 4.0 },
    });

    // Produce a sample (t=0 -> first keyframe).
    track.tick();
    let patch = make_patch_with_track(Arc::clone(&track));

    let mut s = Signal::Track {
        track: "t1".to_string(),
        track_ptr: Weak::new(),
    };
    s.connect(&patch);

    approx_eq(s.get_value_or(-999.0), 2.0, 1e-6);
}
*/

#[test]
fn enum_tag_derive_generates_payload_free_enum() {
    #[derive(modular_derive::EnumTag)]
    enum E<'a, T> {
        A,
        B(u32),
        C { x: i32, y: &'a T },
    }

    let t = 123u8;

    let a: E<'_, u8> = E::A;
    assert_eq!(a.tag(), ETag::A);

    let b: E<'_, u8> = E::B(42);
    assert_eq!(b.tag(), ETag::B);

    let c: E<'_, u8> = E::C { x: -7, y: &t };
    assert_eq!(c.tag(), ETag::C);
}

#[test]
fn message_listener_macro_infers_tags_from_match() {
    struct L;

    impl L {
        fn on_clock(&mut self, _m: &ClockMessages) -> napi::Result<()> {
            Ok(())
        }

        fn on_midi_note(&mut self, _msg: &MidiNoteOn) -> napi::Result<()> {
            Ok(())
        }

        fn on_midi_cc(&mut self, _msg: &MidiControlChange) -> napi::Result<()> {
            Ok(())
        }
    }

    struct LSampleable {
        module: std::cell::UnsafeCell<L>,
    }

    modular_derive::message_handlers!(impl L {
        Clock(m) => L::on_clock,
        MidiNoteOn(msg) => L::on_midi_note,
        MidiCC(msg) => L::on_midi_cc,
    });

    let s = LSampleable {
        module: std::cell::UnsafeCell::new(L),
    };

    assert_eq!(
        s.handled_message_tags(),
        &[
            MessageTag::Clock,
            MessageTag::MidiNoteOn,
            MessageTag::MidiCC,
        ]
    );

    // Dispatch should call the appropriate handler and return Ok.
    s.handle_message(&Message::Clock(ClockMessages::Stop))
        .unwrap();
}

/*
#[test]
fn track_interpolation_linear_step_cubic() {
    // Use keyframes at 0 and 1 so local_t == normalized t.
    let track = Track::new(
        "t".to_string(),
        Signal::Volts { value: -2.5 }, // t = 0.25
        InterpolationType::Linear,
    );
    track.add_keyframe(TrackKeyframe {
        id: "a".to_string(),
        track_id: "t".to_string(),
        time: 0.0,
        signal: Signal::Volts { value: 0.0 },
    });
    track.add_keyframe(TrackKeyframe {
        id: "b".to_string(),
        track_id: "t".to_string(),
        time: 1.0,
        signal: Signal::Volts { value: 8.0 },
    });

    // Linear: 0 + 8*0.25 = 2
    track.configure(Signal::Volts { value: -2.5 }, InterpolationType::Linear);
    track.tick();
    approx_eq(track.get_value_optional().unwrap(), 2.0, 1e-5);

    // Step: always curr
    track.configure(Signal::Volts { value: -2.5 }, InterpolationType::Step);
    track.tick();
    approx_eq(track.get_value_optional().unwrap(), 0.0, 1e-5);

    // Cubic: at t=0.25, easing gives t2=0.125 => 0 + 8*0.125 = 1
    track.configure(
        Signal::Volts { value: -2.5 },
        InterpolationType::CubicIn,
    );
    track.tick();
    approx_eq(track.get_value_optional().unwrap(), 1.0, 1e-5);
}
*/

/*
#[test]
fn track_interpolation_exponential_positive_values() {
    let track = Track::new(
        "t".to_string(),
        Signal::Volts { value: 0.0 }, // t=0.5
        InterpolationType::ExpoIn,
    );
    track.add_keyframe(TrackKeyframe {
        id: "a".to_string(),
        track_id: "t".to_string(),
        time: 0.0,
        signal: Signal::Volts { value: 1.0 },
    });
    track.add_keyframe(TrackKeyframe {
        id: "b".to_string(),
        track_id: "t".to_string(),
        time: 1.0,
        signal: Signal::Volts { value: 4.0 },
    });

    track.tick();
    // 1 * (4/1)^0.5 = 2
    approx_eq(track.get_value_optional().unwrap(), 2.0, 1e-5);
}
*/

/*
#[test]
fn track_clamps_to_first_and_last_keyframes() {
    let track = Track::new(
        "t".to_string(),
        Signal::Volts { value: -10.0 },
        InterpolationType::Linear,
    );
    track.add_keyframe(TrackKeyframe {
        id: "a".to_string(),
        track_id: "t".to_string(),
        time: 0.0,
        signal: Signal::Volts { value: 2.0 },
    });
    track.add_keyframe(TrackKeyframe {
        id: "b".to_string(),
        track_id: "t".to_string(),
        time: 1.0,
        signal: Signal::Volts { value: 4.0 },
    });

    // Below range => first
    track.configure(Signal::Volts { value: -6.0 }, InterpolationType::Linear);
    track.tick();
    approx_eq(track.get_value_optional().unwrap(), 2.0, 1e-6);

    // Above range => last
    track.configure(Signal::Volts { value: 6.0 }, InterpolationType::Linear);
    track.tick();
    approx_eq(track.get_value_optional().unwrap(), 4.0, 1e-6);
}
*/

#[test]
fn connect_noop_for_non_cable_and_non_track_signals() {
    let mut s = Signal::Volts(1.0);
    let patch = make_empty_patch();
    s.connect(&patch);
    approx_eq(s.get_value(), 1.0, 1e-6);
}

#[test]
fn option_signal_deserialize_missing_field() {
    #[derive(Deserialize, Default, Debug)]
    #[serde(default)]
    struct A {
        foo: String,
        sig: Option<Signal>,
    }

    let a: A = serde_json::from_str(r#"{"foo":"bar"}"#).unwrap();
    assert!(a.sig.is_none(), "missing field should deserialize as None");

    println!("{:?}", a);
}
