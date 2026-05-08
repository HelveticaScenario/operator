//! Shared post-execution state: the latest graph JSON, the sliders the DSL
//! declared, the audio channel they push through, etc. Owned as a gpui
//! `Entity<DslState>` so the editor and the controls panel can both see it.

use crossbeam_channel::Sender;
use gpui::{Context, Window};
use modular_core::patch::Patch;

use crate::dsl::build_patch;

#[derive(Clone, Debug)]
pub struct SliderDef {
    pub label: String,
    pub module_id: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

pub struct DslState {
    pub sliders: Vec<SliderDef>,
    /// The most recent graph JSON the DSL emitted, mutated in place when a
    /// slider is dragged so the next `Patch::from_graph` reflects the new
    /// value without re-running JS.
    graph_value: Option<serde_json::Value>,
    sample_rate: f32,
    patch_tx: Option<Sender<Patch>>,
}

impl DslState {
    pub fn new(sample_rate: f32, patch_tx: Option<Sender<Patch>>) -> Self {
        Self {
            sliders: Vec::new(),
            graph_value: None,
            sample_rate,
            patch_tx,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Seed the cached graph without going through `cx.notify()` — used by
    /// the constructor in `main.rs` before the entity is wired to a window.
    pub fn set_graph_value(&mut self, graph: serde_json::Value) {
        self.graph_value = Some(graph);
    }

    pub fn patch_tx(&self) -> Option<&Sender<Patch>> {
        self.patch_tx.as_ref()
    }

    /// Replace the cached graph + sliders after a successful DSL run.
    pub fn update_after_exec(
        &mut self,
        graph: serde_json::Value,
        sliders: Vec<SliderDef>,
        cx: &mut Context<Self>,
    ) {
        self.graph_value = Some(graph);
        self.sliders = sliders;
        cx.notify();
    }

    /// Bump a slider's value by `delta`, clamp to its range, mutate the
    /// stored graph JSON, rebuild a `Patch`, and push it to the audio
    /// thread. No JS involved.
    pub fn bump_slider(
        &mut self,
        label: &str,
        delta: f64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(idx) = self.sliders.iter().position(|s| s.label == label) else {
            return;
        };
        let slider = &mut self.sliders[idx];
        let new_value = (slider.value + delta).clamp(slider.min, slider.max);
        if (new_value - slider.value).abs() < f64::EPSILON {
            return;
        }
        slider.value = new_value;
        let module_id = slider.module_id.clone();
        let new_value_json = serde_json::json!(new_value);

        let Some(graph) = self.graph_value.as_mut() else {
            cx.notify();
            return;
        };

        let mut applied = false;
        if let Some(modules) = graph.get_mut("modules").and_then(|v| v.as_array_mut()) {
            for module in modules.iter_mut() {
                let Some(id) = module.get("id").and_then(|i| i.as_str()) else {
                    continue;
                };
                if id != module_id {
                    continue;
                }
                if let Some(params) = module.get_mut("params").and_then(|p| p.as_object_mut())
                {
                    params.insert("source".to_string(), new_value_json.clone());
                    applied = true;
                }
                break;
            }
        }

        if !applied {
            cx.notify();
            return;
        }

        match build_patch(graph, self.sample_rate) {
            Ok(patch) => {
                if let Some(tx) = self.patch_tx.as_ref() {
                    if let Err(err) = tx.try_send(patch) {
                        eprintln!("[modz] slider send: {err}");
                    }
                }
            }
            Err(err) => eprintln!("[modz] slider rebuild Patch: {err}"),
        }
        cx.notify();
    }
}

