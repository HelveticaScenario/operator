//! Module id constraints: uniqueness across the patch and the reservations
//! well-known engine ids place on user graphs.

use super::{ValidationError, format_module_location};
use modular_core::types::{PatchGraph, WellKnownModule};

/// Constraint a well-known module id places on a patch graph: `Some(None)`
/// means the id is engine-injected and may not appear in a graph at all;
/// `Some(Some(t))` means the id may only be used by a module of type `t`;
/// `None` means the id is unreserved.
fn reserved_id_requirement(id: &str) -> Option<Option<&'static str>> {
    if id == WellKnownModule::HiddenAudioIn.id() {
        Some(None)
    } else if id == WellKnownModule::RootClock.id() {
        Some(Some("_clock"))
    } else if id == WellKnownModule::RootOutput.id() || id == WellKnownModule::RootInput.id() {
        Some(Some("$signal"))
    } else {
        None
    }
}

/// The audio thread stores modules in a HashMap keyed by id, so a duplicate
/// id would silently replace another module. Ids of engine-managed modules
/// are likewise only valid with their canonical type (HIDDEN_AUDIO_IN is
/// injected Rust-side and must never appear in a graph at all — a module
/// using that id would replace the host audio input).
pub(super) fn validate_module_ids(patch: &PatchGraph, errors: &mut Vec<ValidationError>) {
    let mut seen_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for module in &patch.modules {
        let location_str = format_module_location(module);

        if !seen_ids.insert(module.id.as_str()) {
            errors.push(ValidationError {
                field: "id".to_string(),
                message: format!("Duplicate module id '{}'", module.id),
                location: Some(location_str.clone()),
                expected_type: None,
                actual_value: None,
            });
        }

        match reserved_id_requirement(&module.id) {
            Some(None) => errors.push(ValidationError {
                field: "id".to_string(),
                message: format!("Module id '{}' is reserved for the engine", module.id),
                location: Some(location_str.clone()),
                expected_type: None,
                actual_value: None,
            }),
            Some(Some(required_type)) if module.module_type != required_type => {
                errors.push(ValidationError {
                    field: "id".to_string(),
                    message: format!(
                        "Module id '{}' is reserved for a {} module",
                        module.id, required_type
                    ),
                    location: Some(location_str.clone()),
                    expected_type: None,
                    actual_value: None,
                });
            }
            _ => {}
        }
    }
}
