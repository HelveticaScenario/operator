//! deno_core-backed JavaScript runtime for executing the Modular DSL.
//!
//! Stage 1: thin wrapper around `JsRuntime` that can evaluate a script and
//! return a JSON value. Wired into both the Cmd-S handler in the GUI and the
//! `--emit-graph` CLI mode. The full runtime will eventually:
//!
//! * Bundle `src/main/dsl/{executor,factories,GraphBuilder}.ts` into V8.
//! * Expose ops (`op_emit_patch`, `op_argument_spans`, `op_load_wav`, ...) that
//!   the bundled JS calls into to push results back to Rust.
//!
//! For now this just wraps `execute_script` so the rest of the binary can
//! exercise the V8 boot path and we can iterate on op surface incrementally.

use deno_core::{JsRuntime, RuntimeOptions, serde_v8, v8};

pub struct DslRuntime {
    runtime: JsRuntime,
}

impl DslRuntime {
    pub fn new() -> Self {
        Self {
            runtime: JsRuntime::new(RuntimeOptions::default()),
        }
    }

    /// Evaluate a JS expression and return its value as JSON.
    pub fn eval(
        &mut self,
        name: &'static str,
        source: String,
    ) -> Result<serde_json::Value, String> {
        let global = self
            .runtime
            .execute_script(name, source)
            .map_err(|e| format!("execute_script: {e:?}"))?;
        deno_core::scope!(scope, &mut self.runtime);
        let local = v8::Local::new(scope, global);
        serde_v8::from_v8::<serde_json::Value>(scope, local)
            .map_err(|e| format!("serde_v8: {e:?}"))
    }
}

impl Default for DslRuntime {
    fn default() -> Self {
        Self::new()
    }
}
