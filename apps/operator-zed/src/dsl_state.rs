//! Shared post-execution state: the latest graph JSON, the sliders the DSL
//! declared, the audio channel they push through, etc. Owned as a gpui
//! `Entity<DslState>` so the editor and the controls panel can both see it.

use std::collections::VecDeque;
use std::sync::Arc;

use crossbeam_channel::Sender;
use gpui::{Context, Window};
use modular_core::patch::Patch;
use parking_lot::Mutex;

use crate::dsl::build_patch;

/// Per-scope-channel ring buffer. Audio thread pushes one sample per audio
/// frame into `samples`; UI thread reads it on each render.
#[derive(Clone)]
pub struct ScopeTarget {
    pub label: String,
    pub module_id: String,
    pub port_name: String,
    pub channel: u32,
    pub range: (f64, f64),
    /// 1-based source line of the originating `.scope()` call. Used by the
    /// inline-block renderer to anchor the waveform under that line.
    pub source_line: Option<u32>,
    pub samples: Arc<Mutex<VecDeque<f32>>>,
}

impl ScopeTarget {
    pub fn new(
        module_id: String,
        port_name: String,
        channel: u32,
        range: (f64, f64),
        source_line: Option<u32>,
        capacity: usize,
    ) -> Self {
        Self {
            label: format!("{module_id}.{port_name}[{channel}]"),
            module_id,
            port_name,
            channel,
            range,
            source_line,
            samples: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
        }
    }
}

/// Capacity (samples) for each scope ring. ~250ms at 48 kHz.
pub const SCOPE_RING_CAPACITY: usize = 12_288;

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
    /// Live scope targets, shared with the audio thread. Mutated on each DSL
    /// run; both the cpal callback and the ScopesView read it.
    pub scope_targets: Arc<Mutex<Vec<ScopeTarget>>>,
}

impl DslState {
    pub fn new(
        sample_rate: f32,
        patch_tx: Option<Sender<Patch>>,
        scope_targets: Arc<Mutex<Vec<ScopeTarget>>>,
    ) -> Self {
        Self {
            sliders: Vec::new(),
            graph_value: None,
            sample_rate,
            patch_tx,
            scope_targets,
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

    /// Read tempo + time signature from the cached graph's ROOT_CLOCK module.
    /// Returns (tempo BPM, numerator, denominator). Falls back to (120, 4, 4)
    /// if the graph isn't populated yet.
    pub fn clock_info(&self) -> (f64, u32, u32) {
        let Some(graph) = self.graph_value.as_ref() else {
            return (120.0, 4, 4);
        };
        let modules = graph.get("modules").and_then(|m| m.as_array());
        let Some(modules) = modules else {
            return (120.0, 4, 4);
        };
        for module in modules.iter() {
            if module.get("id").and_then(|i| i.as_str()) == Some("ROOT_CLOCK") {
                let params = module.get("params");
                let tempo = params
                    .and_then(|p| p.get("tempo"))
                    .and_then(|t| t.as_f64())
                    .unwrap_or(120.0);
                let numerator = params
                    .and_then(|p| p.get("numerator"))
                    .and_then(|n| n.as_u64())
                    .unwrap_or(4) as u32;
                let denominator = params
                    .and_then(|p| p.get("denominator"))
                    .and_then(|n| n.as_u64())
                    .unwrap_or(4) as u32;
                return (tempo, numerator, denominator);
            }
        }
        (120.0, 4, 4)
    }

    pub fn patch_tx(&self) -> Option<&Sender<Patch>> {
        self.patch_tx.as_ref()
    }

    /// Replace the cached graph + sliders + scope targets after a successful
    /// DSL run. Existing scope ring buffers are dropped along with the old
    /// targets — callers run this on the main thread, audio holds the Arc<Mutex>
    /// so the swap is atomic from its perspective.
    pub fn update_after_exec(
        &mut self,
        graph: serde_json::Value,
        sliders: Vec<SliderDef>,
        scopes: Vec<ScopeTarget>,
        cx: &mut Context<Self>,
    ) {
        self.graph_value = Some(graph);
        self.sliders = sliders;
        *self.scope_targets.lock() = scopes;
        cx.notify();
    }

    /// Bump a slider's value by `delta`, clamp to its range, mutate the
    /// stored graph JSON, rebuild a `Patch`, and push it to the audio
    /// thread. No JS involved. Returns `Some(new_value)` if the value
    /// changed, `None` otherwise.
    pub fn bump_slider(
        &mut self,
        label: &str,
        delta: f64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<f64> {
        let idx = self.sliders.iter().position(|s| s.label == label)?;
        let slider = &mut self.sliders[idx];
        let new_value = (slider.value + delta).clamp(slider.min, slider.max);
        if (new_value - slider.value).abs() < f64::EPSILON {
            return None;
        }
        slider.value = new_value;
        let module_id = slider.module_id.clone();
        let new_value_json = serde_json::json!(new_value);

        let Some(graph) = self.graph_value.as_mut() else {
            cx.notify();
            return Some(new_value);
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
            return Some(new_value);
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
        Some(new_value)
    }
}

