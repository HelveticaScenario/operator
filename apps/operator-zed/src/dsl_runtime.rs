//! deno_core-backed JavaScript runtime that executes the Modular DSL.
//!
//! The bundled `dsl_runtime.js` is produced at build time by `build.rs`
//! (esbuild over `apps/operator-zed/dsl/entry.ts`, which pulls in
//! `src/main/dsl/*.ts` with two aliases:
//!   - `@modular/core` -> `dsl/modular_core_shim.ts`
//!   - `src/main/dsl/analyzeSource` -> `dsl/analyze_source_stub.ts`
//! The bundle exposes `globalThis.modz_executePatchScript(source, schemas)`.
//!
//! `DslRuntime::new()` boots a `JsRuntime`, registers Rust ops the shim
//! calls into (`deriveChannelCount`, `getReservedOutputNames`, ...), then
//! evaluates the bundle so the entry function is reachable. Per-execution
//! the runtime stages the source and schemas into globals and invokes the
//! entry via `execute_script`, deserializing the JS-side return value into
//! a `serde_json::Value`.

use deno_core::{Extension, JsRuntime, OpDecl, RuntimeOptions, op2, serde_v8, v8};
use modular_core::dsp::{get_params_deserializers, schema as dsp_schemas};
use modular_core::params::extract_argument_spans;

const BUNDLE_SOURCE: &str = include_str!(concat!(env!("OUT_DIR"), "/dsl_runtime.js"));

/// op_log with a Modular-Core derived channel count for one module's params.
#[op2]
#[serde]
fn op_modz_derive_channel_count(
    #[string] module_type: String,
    #[serde] params: serde_json::Value,
) -> serde_json::Value {
    let (stripped, _argument_spans) = extract_argument_spans(params);
    let deserializers = get_params_deserializers();
    let Some(deserializer) = deserializers.get(&module_type) else {
        return serde_json::json!({
            "channelCount": null,
            "errors": [{
                "message": format!("No params deserializer for module type: {module_type}"),
                "params": [],
            }],
        });
    };
    match deserializer(stripped) {
        Ok(cached) => serde_json::json!({
            "channelCount": cached.channel_count,
            "errors": null,
        }),
        Err(err) => {
            let errors: Vec<serde_json::Value> = err
                .into_errors()
                .into_iter()
                .map(|e| {
                    serde_json::json!({
                        "message": e.message,
                        "params": if e.field.is_empty() { vec![] } else { vec![e.field] },
                    })
                })
                .collect();
            serde_json::json!({
                "channelCount": null,
                "errors": errors,
            })
        }
    }
}

#[op2]
#[serde]
fn op_modz_reserved_output_names() -> Vec<String> {
    // Mirrors crates/reserved_output_names.rs (the single source of truth used
    // by the napi build). Kept inline here to avoid pulling crates/modular
    // (which is the cdylib napi addon) into the operator-zed dep tree.
    [
        "builder",
        "moduleId",
        "portName",
        "channel",
        "amplitude",
        "amp",
        "exp",
        "gain",
        "shift",
        "scope",
        "out",
        "outMono",
        "pipe",
        "pipeMix",
        "toString",
        "minValue",
        "maxValue",
        "range",
        "items",
        "length",
        "set",
        "constructor",
        "prototype",
        "__proto__",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

#[op2(fast)]
fn op_modz_log(#[string] level: &str, #[string] msg: &str) {
    eprintln!("[modz/js {level}] {msg}");
}

const MODZ_OPS: &[OpDecl] = &[
    op_modz_derive_channel_count(),
    op_modz_reserved_output_names(),
    op_modz_log(),
];

pub struct DslRuntime {
    runtime: JsRuntime,
    schemas: serde_json::Value,
}

impl DslRuntime {
    pub fn new() -> Result<Self, String> {
        let extension = Extension {
            name: "modz_dsl",
            ops: std::borrow::Cow::Borrowed(MODZ_OPS),
            ..Default::default()
        };

        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![extension],
            ..Default::default()
        });

        // Make the bundled DSL globals available.
        runtime
            .execute_script("modz:dsl_runtime.js", BUNDLE_SOURCE)
            .map_err(|err| format!("loading dsl_runtime.js: {err:?}"))?;

        let schemas = serde_json::to_value(dsp_schemas())
            .map_err(|err| format!("serializing schemas: {err}"))?;

        Ok(Self { runtime, schemas })
    }

    /// Quick health check used by --emit-graph and the cmd-S handler before
    /// the bundle is fully wired. Calls a tiny inline script and returns the
    /// JSON value it produced.
    pub fn probe(&mut self) -> Result<serde_json::Value, String> {
        self.eval_script("modz:probe", "({ runtime: 'deno_core', version: 1 })")
    }

    /// Execute a DSL source string through the bundled `modz_executePatchScript`
    /// entry. Returns the parsed envelope `{ ok, value | error }`.
    pub fn execute(&mut self, source: &str) -> Result<serde_json::Value, String> {
        // Stage source + schemas as globals so the call can reference them.
        let setup = format!(
            "globalThis.__modz_source = {};\nglobalThis.__modz_schemas = {};\n0",
            json_string_literal(source),
            self.schemas,
        );
        self.eval_script("modz:stage", &setup)?;
        self.eval_script(
            "modz:exec",
            "globalThis.modz_executePatchScript(globalThis.__modz_source, globalThis.__modz_schemas)",
        )
    }

    fn eval_script(
        &mut self,
        name: &'static str,
        source: &str,
    ) -> Result<serde_json::Value, String> {
        let global = self
            .runtime
            .execute_script(name, source.to_string())
            .map_err(|err| format!("execute_script({name}): {err:?}"))?;
        deno_core::scope!(scope, &mut self.runtime);
        let local = v8::Local::new(scope, global);
        serde_v8::from_v8::<serde_json::Value>(scope, local)
            .map_err(|err| format!("serde_v8({name}): {err:?}"))
    }
}

/// Encode `s` as a JSON string literal so it can be safely interpolated into
/// JS source.
fn json_string_literal(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}
