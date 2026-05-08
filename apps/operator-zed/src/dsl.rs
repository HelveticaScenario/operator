//! Shared DSL execution glue: runs source through `DslRuntime`, builds a
//! `Patch` from the resulting graph, and pushes it to the audio thread.

use modular_core::patch::Patch;
use modular_core::types::{ModuleIdRemap, ModuleState, PatchGraph};
use serde::Deserialize;

use crate::dsl_runtime::DslRuntime;
use crate::dsl_state::{SCOPE_RING_CAPACITY, ScopeTarget, SliderDef};

pub struct DslExecution {
    pub graph_value: serde_json::Value,
    pub sliders: Vec<SliderDef>,
    pub scopes: Vec<ScopeTarget>,
    pub patch: Patch,
    pub module_count: usize,
}

/// Execute the DSL once, returning the resulting graph JSON, the slider
/// definitions, and a freshly built `Patch`. Caller decides where to send
/// the patch and how to cache the rest.
pub fn run(source: &str, sample_rate: f32) -> Result<DslExecution, String> {
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

    let sliders_value = envelope
        .pointer("/value/sliders")
        .cloned()
        .unwrap_or(serde_json::Value::Array(Vec::new()));
    let sliders = parse_sliders(&sliders_value);

    let scopes_value = graph_value
        .get("scopes")
        .cloned()
        .unwrap_or(serde_json::Value::Array(Vec::new()));
    let scopes = parse_scopes(&scopes_value);

    let patch = build_patch(&graph_value, sample_rate)?;
    let module_count = graph_value
        .get("modules")
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    Ok(DslExecution {
        graph_value,
        sliders,
        scopes,
        patch,
        module_count,
    })
}

/// Deserialize a graph JSON value (already sanitized) into a `PatchGraph` and
/// build a `Patch`. Used by both the JS-driven cmd-S path and the slider
/// drag path that mutates an existing graph in place.
pub fn build_patch(
    graph_value: &serde_json::Value,
    sample_rate: f32,
) -> Result<Patch, String> {
    // `PatchGraph` doesn't derive Deserialize itself (napi-derive owns the
    // shape on the JS boundary), so go through a mirror. We don't deserialize
    // Scope into modular_core's struct because Patch::from_graph ignores
    // graph.scopes anyway and the field uses snake_case keys instead of the
    // JS-emitted camelCase. We keep the scopes empty here and parse them
    // separately for the UI in `parse_scopes`.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeserPatchGraph {
        modules: Vec<ModuleState>,
        #[serde(default)]
        module_id_remaps: Option<Vec<ModuleIdRemap>>,
    }
    let mirror: DeserPatchGraph = serde_json::from_value(strip_scopes(graph_value.clone()))
        .map_err(|err| format!("PatchGraph deserialize: {err}"))?;
    let graph = PatchGraph {
        modules: mirror.modules,
        module_id_remaps: mirror.module_id_remaps,
        scopes: Vec::new(),
    };
    Patch::from_graph(&graph, sample_rate)
        .map_err(|err| format!("Patch::from_graph: {err}"))
}

fn strip_scopes(mut graph: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = graph.as_object_mut() {
        obj.remove("scopes");
    }
    graph
}

fn parse_scopes(value: &serde_json::Value) -> Vec<ScopeTarget> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    let mut targets = Vec::new();
    for scope in arr.iter() {
        let channels = scope
            .get("channels")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let range = scope
            .get("range")
            .and_then(|r| r.as_array())
            .and_then(|arr| Some((arr.first()?.as_f64()?, arr.get(1)?.as_f64()?)))
            .unwrap_or((-5.0, 5.0));
        for ch in channels.iter() {
            let Some(module_id) = ch.get("moduleId").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(port_name) = ch.get("portName").and_then(|v| v.as_str()) else {
                continue;
            };
            let channel = ch.get("channel").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            targets.push(ScopeTarget::new(
                module_id.to_string(),
                port_name.to_string(),
                channel,
                range,
                SCOPE_RING_CAPACITY,
            ));
        }
    }
    targets
}

fn parse_sliders(value: &serde_json::Value) -> Vec<SliderDef> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|entry| {
            let label = entry.get("label")?.as_str()?.to_string();
            let module_id = entry.get("moduleId")?.as_str()?.to_string();
            let value = entry.get("value")?.as_f64()?;
            let min = entry.get("min")?.as_f64()?;
            let max = entry.get("max")?.as_f64()?;
            Some(SliderDef {
                label,
                module_id,
                value,
                min,
                max,
            })
        })
        .collect()
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
