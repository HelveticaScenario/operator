// Integration test: verify {Name}BlockOutputs is generated alongside #[derive(Outputs)].
//
// The proc-macro expands to `crate::types::...`, `crate::block_port::...`, `crate::poly::...`
// so we provide those module shims here.

mod types {
    pub use modular_core::types::*;
}
mod block_port {
    pub use modular_core::block_port::*;
}
mod poly {
    pub use modular_core::poly::*;
}

use modular_core::poly::PolyOutput;

#[derive(modular_derive::Outputs)]
struct SimpleOutputs {
    #[output("value", "A value", default)]
    value: f32,
    #[output("poly", "Poly out")]
    poly: PolyOutput,
}

#[test]
fn block_outputs_struct_exists() {
    let bo = SimpleBlockOutputs::new(4);
    // Fresh buffer returns 0.0 at every index.
    assert_eq!(bo.get_at(0, 0, 0), 0.0);
    assert_eq!(bo.get_at(1, 2, 3), 0.0);
}

#[test]
fn copy_from_inner_fills_block_outputs() {
    let inner = SimpleOutputs {
        value: 2.5,
        poly: PolyOutput::mono(1.0),
    };
    let mut bo = SimpleBlockOutputs::new(4);
    bo.copy_from_inner(&inner, 2);
    let value_idx = SimpleBlockOutputs::port_index("value").unwrap();
    let poly_idx = SimpleBlockOutputs::port_index("poly").unwrap();
    assert!((bo.get_at(value_idx, 0, 2) - 2.5).abs() < 1e-6);
    assert!((bo.get_at(poly_idx, 0, 2) - 1.0).abs() < 1e-6);
}

#[test]
fn port_index_resolves_known_ports() {
    assert_eq!(SimpleBlockOutputs::port_index("value"), Some(0));
    assert_eq!(SimpleBlockOutputs::port_index("poly"), Some(1));
}

#[test]
fn port_index_returns_none_for_unknown() {
    assert_eq!(SimpleBlockOutputs::port_index("nonexistent"), None);
    assert_eq!(SimpleBlockOutputs::port_index(""), None);
}

#[test]
fn get_at_reads_correct_port() {
    let inner = SimpleOutputs {
        value: 7.5,
        poly: PolyOutput::mono(3.5),
    };
    let mut bo = SimpleBlockOutputs::new(4);
    bo.copy_from_inner(&inner, 1);
    assert!((bo.get_at(0, 0, 1) - 7.5).abs() < 1e-6); // value
    assert!((bo.get_at(1, 0, 1) - 3.5).abs() < 1e-6); // poly
}

#[test]
fn get_at_out_of_range_returns_zero() {
    let bo = SimpleBlockOutputs::new(4);
    assert_eq!(bo.get_at(99, 0, 0), 0.0);
}
