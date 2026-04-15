use std::collections::HashMap;
use std::sync::{Arc, Weak};

use napi::Result;
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

    fn get_sample(&self, port: &str, channel: usize) -> Result<f32> {
        self.get_poly_sample(port).map(|p| p.get_cycling(channel))
    }

    fn get_module_type(&self) -> &str {
        &self.module_type
    }

    fn connect(&self, _patch: &Patch) {
        println!("Connecting DummySampleable {}", self.id);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

#[test]
fn connect_noop_for_non_cable_and_non_track_signals() {
    let mut s = Signal::Volts(1.0);
    let patch = make_empty_patch();
    s.connect(&patch);
    approx_eq(s.get_value(), 1.0, 1e-6);
}
