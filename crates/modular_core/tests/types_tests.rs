use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use napi::Result;
use serde_json::json;

use modular_core::patch::Patch;
use modular_core::types::{
    Buffer, BufferData, ClockMessages, Connect, Message, MessageHandler, MessageTag,
    MidiControlChange, MidiNoteOn, Sampleable, Signal, SignalExt,
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

fn make_patch_with_sampleable(sampleable: Box<dyn Sampleable>) -> Patch {
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
            resolved,
            channel,
            ..
        } => {
            assert_eq!(module, "m1");
            assert_eq!(port, "out");
            assert_eq!(channel, 0);
            assert!(resolved.is_none());
        }
        other => panic!("expected Signal::Cable, got {other:?}"),
    }
}

#[test]
fn signal_cable_connect_and_read() {
    let sampleable: Box<dyn Sampleable> =
        Box::new(DummySampleable::new("m1", "dummy", [("out", 3.5)]));
    let patch = make_patch_with_sampleable(sampleable);

    let mut s = Signal::cable("m1", "out", 0);

    // Before connect, cable reads 0.0 because the cache is unresolved.
    approx_eq(s.get_value(), 0.0, 1e-6);

    s.connect(&patch);

    match &s {
        Signal::Cable { resolved, .. } => assert!(resolved.is_some()),
        other => panic!("expected Signal::Cable, got {other:?}"),
    }

    approx_eq(s.get_value(), 3.5, 1e-6);
}

#[test]
fn signal_cable_reconnect_to_missing_source_clears_resolved_and_reads_zero() {
    let sampleable: Box<dyn Sampleable> =
        Box::new(DummySampleable::new("m1", "dummy", [("out", 3.5)]));
    let patch = make_patch_with_sampleable(sampleable);
    let empty_patch = make_empty_patch();

    let mut s = Signal::cable("m1", "out", 0);

    s.connect(&patch);
    approx_eq(s.get_value(), 3.5, 1e-6);

    s.connect(&empty_patch);

    match &s {
        Signal::Cable { resolved, .. } => assert!(resolved.is_none()),
        other => panic!("expected Signal::Cable, got {other:?}"),
    }

    approx_eq(s.get_value(), 0.0, 1e-6);
}

#[test]
fn signal_cable_reconnect_to_replacement_source_rebinds_resolved_and_reads_new_value() {
    let first: Box<dyn Sampleable> = Box::new(DummySampleable::new("m1", "dummy", [("out", 3.5)]));
    let second: Box<dyn Sampleable> =
        Box::new(DummySampleable::new("m1", "dummy", [("out", 7.25)]));
    let first_patch = make_patch_with_sampleable(first);
    let second_patch = make_patch_with_sampleable(second);

    let mut s = Signal::cable("m1", "out", 0);

    s.connect(&first_patch);
    let first_resolved = match &s {
        Signal::Cable { resolved, .. } => {
            approx_eq(s.get_value(), 3.5, 1e-6);
            *resolved
        }
        other => panic!("expected Signal::Cable, got {other:?}"),
    };

    s.connect(&second_patch);

    match &s {
        Signal::Cable { resolved, .. } => {
            assert!(resolved.is_some());
            assert_ne!(*resolved, first_resolved);
        }
        other => panic!("expected Signal::Cable, got {other:?}"),
    }

    approx_eq(s.get_value(), 7.25, 1e-6);
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

#[test]
fn collect_cables_volts_emits_nothing() {
    use modular_core::types::Connect;
    let s = Signal::Volts(1.5);
    let mut sink = Vec::new();
    s.collect_cables(&mut sink);
    assert!(sink.is_empty());
}

#[test]
fn collect_cables_cable_emits_module_id() {
    use modular_core::types::Connect;
    let s = Signal::cable("OSC1", "out", 0);
    let mut sink = Vec::new();
    s.collect_cables(&mut sink);
    assert_eq!(sink, vec!["OSC1".to_string()]);
}

#[test]
fn collect_cables_container_forwarding() {
    use modular_core::types::Connect;
    let signals: Vec<Signal> = vec![
        Signal::Volts(0.0),
        Signal::cable("A", "out", 0),
        Signal::cable("B", "trig", 1),
    ];
    let mut sink = Vec::new();
    signals.collect_cables(&mut sink);
    assert_eq!(sink, vec!["A".to_string(), "B".to_string()]);
}

#[test]
fn collect_cables_primitive_noop() {
    use modular_core::types::Connect;
    let mut sink = Vec::new();
    1.5f32.collect_cables(&mut sink);
    42usize.collect_cables(&mut sink);
    "foo".to_string().collect_cables(&mut sink);
    assert!(sink.is_empty());
}

#[test]
fn collect_cables_buffer_emits_source_module() {
    use modular_core::types::{Buffer, Connect};
    let buf = Buffer::new("RECORDER".into(), "out".into(), 2);
    let mut sink = Vec::new();
    buf.collect_cables(&mut sink);
    assert_eq!(sink, vec!["RECORDER".to_string()]);
}

#[test]
fn collect_cables_table_with_signal_cable() {
    use modular_core::poly::PolySignal;
    use modular_core::types::{Connect, Table};
    let table = Table::Bend {
        amount: PolySignal::mono(Signal::cable("LFO", "out", 0)),
    };
    let mut sink = Vec::new();
    table.collect_cables(&mut sink);
    assert_eq!(sink, vec!["LFO".to_string()]);
}

#[test]
fn collect_cables_table_pipe_recurses() {
    use modular_core::poly::PolySignal;
    use modular_core::types::{Connect, Table};
    // Pipe(Bend{amount: cable→A}, Sync{ratio: cable→B})
    let table = Table::Pipe {
        first: Box::new(Table::Bend {
            amount: PolySignal::mono(Signal::cable("A", "out", 0)),
        }),
        second: Box::new(Table::Sync {
            ratio: PolySignal::mono(Signal::cable("B", "out", 0)),
        }),
    };
    let mut sink = Vec::new();
    table.collect_cables(&mut sink);
    assert_eq!(sink, vec!["A".to_string(), "B".to_string()]);
}

#[test]
fn collect_cables_table_identity_emits_nothing() {
    use modular_core::types::{Connect, Table};
    let mut sink = Vec::new();
    Table::Identity.collect_cables(&mut sink);
    assert!(sink.is_empty());
}

#[test]
fn inject_index_ptr_sets_field_on_cable() {
    use modular_core::types::Connect;
    use std::cell::Cell;

    let cell = Cell::new(7usize);
    let ptr: *const Cell<usize> = &cell;

    let mut s = Signal::cable("m1", "out", 0);
    s.inject_index_ptr(ptr);

    match &s {
        Signal::Cable { index_ptr, .. } => {
            assert!(!index_ptr.is_null());
            // Round-trip through the pointer to confirm it points at our cell.
            let read = unsafe { (*(*index_ptr)).get() };
            assert_eq!(read, 7);
        }
        _ => panic!("expected Signal::Cable"),
    }
}

#[test]
fn inject_index_ptr_noop_on_volts() {
    use modular_core::types::Connect;
    use std::cell::Cell;

    let cell = Cell::new(0usize);
    let mut s = Signal::Volts(2.5);
    s.inject_index_ptr(&cell);
    // Volts has no `index_ptr` field — call must be a no-op (just verify it
    // doesn't panic, and that the value is preserved).
    match s {
        Signal::Volts(v) => assert_eq!(v, 2.5),
        _ => panic!("expected Signal::Volts"),
    }
}

#[test]
fn inject_index_ptr_through_polysignal_reaches_inner_cables() {
    use modular_core::poly::PolySignal;
    use modular_core::types::Connect;
    use std::cell::Cell;

    let cell = Cell::new(42usize);
    let ptr: *const Cell<usize> = &cell;

    let mut poly = PolySignal::poly(&[
        Signal::cable("A", "out", 0),
        Signal::Volts(1.0),
        Signal::cable("B", "trig", 1),
    ]);
    poly.inject_index_ptr(ptr);

    // Both cable channels should now point at our cell.
    for ch in 0..poly.channels() {
        if let Some(Signal::Cable { index_ptr, .. }) = poly.get(ch) {
            assert!(!index_ptr.is_null());
            assert_eq!(unsafe { (*(*index_ptr)).get() }, 42);
        }
    }
}

#[test]
fn inject_index_ptr_through_table_pipe_reaches_nested_cables() {
    use modular_core::poly::PolySignal;
    use modular_core::types::{Connect, Table};
    use std::cell::Cell;

    let cell = Cell::new(99usize);
    let ptr: *const Cell<usize> = &cell;

    let mut table = Table::Pipe {
        first: Box::new(Table::Bend {
            amount: PolySignal::mono(Signal::cable("A", "out", 0)),
        }),
        second: Box::new(Table::Sync {
            ratio: PolySignal::mono(Signal::cable("B", "out", 0)),
        }),
    };
    table.inject_index_ptr(ptr);

    // Reach into nested cables and verify both got the pointer.
    if let Table::Pipe { first, second } = &table {
        for inner in [first.as_ref(), second.as_ref()] {
            let cable = match inner {
                Table::Bend { amount } => amount.get(0),
                Table::Sync { ratio } => ratio.get(0),
                _ => panic!("unexpected inner variant"),
            };
            if let Some(Signal::Cable { index_ptr, .. }) = cable {
                assert!(!index_ptr.is_null());
                assert_eq!(unsafe { (*(*index_ptr)).get() }, 99);
            } else {
                panic!("expected Cable inside table");
            }
        }
    }
}
