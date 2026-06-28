//! Shared integration-test helpers for `modular_core`.

use std::collections::HashMap;

use modular_core::params::{DeserializedParams, strip_argument_spans};
use modular_core::patch::Patch;
use modular_core::types::{PatchGraph, ProcessingMode};

/// Build a fully connected `Patch` from a `PatchGraph` for testing.
/// Mirrors what the app does for a fresh patch with no prior state to transfer
pub fn from_graph(
    graph: &PatchGraph,
    sample_rate: f32,
    block_size: usize,
    mode_map: &HashMap<String, ProcessingMode>,
) -> Result<Patch, String> {
    let deserializers = modular_core::dsp::get_params_deserializers();
    let mut modules = Vec::with_capacity(graph.modules.len());
    for module_state in &graph.modules {
        let deserializer = deserializers
            .get(&module_state.module_type)
            .ok_or_else(|| format!("Unknown module type: {}", module_state.module_type))?;
        let cached =
            deserializer(strip_argument_spans(module_state.params.clone())).map_err(|e| {
                format!(
                    "Failed to deserialize params for {}: {}",
                    module_state.id, e
                )
            })?;
        let mode = mode_map
            .get(&module_state.id)
            .copied()
            .unwrap_or(ProcessingMode::Block);
        modules.push((
            module_state.id.clone(),
            module_state.module_type.clone(),
            DeserializedParams {
                params: cached.params,
                channel_count: cached.channel_count,
            },
            mode,
        ));
    }

    let mut patch = Patch::new();
    patch.insert_modules(modules, sample_rate, block_size)?;
    patch.rebuild_message_listeners();
    patch.connect_all();
    Ok(patch)
}
