//! Shared DSL execution glue: runs source through `DslRuntime`, builds a
//! `Patch` from the resulting graph, and pushes it to the audio thread.

use crossbeam_channel::Sender;
use modular_core::patch::Patch;
use modular_core::types::{ModuleIdRemap, ModuleState, PatchGraph, Scope};
use serde::Deserialize;

use crate::dsl_runtime::DslRuntime;

pub fn run_and_send_patch(
    source: &str,
    sample_rate: f32,
    patch_tx: Option<&Sender<Patch>>,
) -> Result<(), String> {
    let mut runtime =
        DslRuntime::new().map_err(|err| format!("DslRuntime init failed: {err}"))?;
    let envelope = runtime
        .execute(source)
        .map_err(|err| format!("DSL execute failed: {err}"))?;

    if envelope.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!(
            "DSL error: {}",
            envelope
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)"),
        ));
    }

    let mut graph_value = envelope
        .pointer("/value/patch")
        .ok_or_else(|| "DSL result missing /value/patch".to_string())?
        .clone();

    sanitize_graph_for_modular_core(&mut graph_value);

    // `PatchGraph` itself doesn't derive Deserialize (napi-derive owns the
    // shape on the JS boundary), so deserialize through a mirror struct
    // and copy across.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeserPatchGraph {
        modules: Vec<ModuleState>,
        #[serde(default)]
        module_id_remaps: Option<Vec<ModuleIdRemap>>,
        #[serde(default)]
        scopes: Vec<Scope>,
    }
    let mirror: DeserPatchGraph = serde_json::from_value(graph_value)
        .map_err(|err| format!("PatchGraph deserialize: {err}"))?;
    let graph = PatchGraph {
        modules: mirror.modules,
        module_id_remaps: mirror.module_id_remaps,
        scopes: mirror.scopes,
    };
    let module_count = graph.modules.len();

    let patch = Patch::from_graph(&graph, sample_rate)
        .map_err(|err| format!("Patch::from_graph: {err}"))?;

    if let Some(tx) = patch_tx {
        if let Err(err) = tx.try_send(patch) {
            return Err(format!("audio channel send: {err}"));
        }
        eprintln!("[modz] DSL ok — {module_count} modules; patch sent to audio");
    } else {
        eprintln!("[modz] DSL ok — {module_count} modules; no audio channel");
    }
    Ok(())
}

/// Strip params the DSL emits for napi's `apply_patch` that the modular_core
/// deserializer (with `deny_unknown_fields`) rejects. Until the napi addon and
/// operator-zed share the same deserializer surface, drop the known extras.
fn sanitize_graph_for_modular_core(graph: &mut serde_json::Value) {
    let Some(modules) = graph.get_mut("modules").and_then(|m| m.as_array_mut()) else {
        return;
    };
    for module in modules.iter_mut() {
        if let Some(params) = module.get_mut("params").and_then(|p| p.as_object_mut()) {
            // ROOT_CLOCK: GraphBuilder.ts:786 stamps `tempoSet` on the params
            // for the napi `apply_patch` path; ClockParams in modular_core has
            // `deny_unknown_fields`.
            params.remove("tempoSet");
        }
    }
}
