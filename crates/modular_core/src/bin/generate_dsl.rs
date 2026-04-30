//! `generate-dsl` — write @modular/dsl generated artifacts from `dsp::schema()`.

use modular_core::codegen::category::all_categories;
use modular_core::codegen::factory_renderer::{render_category, render_index};
use modular_core::codegen::metadata_renderer;
use modular_core::codegen::reserved_names_renderer;
use modular_core::codegen::type_resolver::schema_to_type_expr;
use modular_core::codegen::writer::{run_prettier, write_if_changed};
use modular_core::dsp::schema;
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DEFAULT_OUT_DIR: &str = "crates/modular/dsl/src/generated";

fn main() -> ExitCode {
    let mut out_dir: Option<PathBuf> = None;
    let mut print_debug = false;
    let mut no_format = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let val = match args.next() {
                    Some(v) => v,
                    None => {
                        eprintln!("--out-dir requires a value");
                        return ExitCode::from(2);
                    }
                };
                out_dir = Some(PathBuf::from(val));
            }
            "--print-resolver-debug" => print_debug = true,
            "--no-format" => no_format = true,
            "--help" | "-h" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_help();
                return ExitCode::from(2);
            }
        }
    }

    if print_debug {
        let modules = schema();
        for m in modules.iter().take(3) {
            let raw = serde_json::to_value(&m.params_schema)
                .expect("failed to serialize params schema");
            match schema_to_type_expr(&raw, &raw) {
                Ok(t) => println!("{}: {}", m.name, t),
                Err(e) => eprintln!("{}: ERROR {}", m.name, e),
            }
        }
        return ExitCode::SUCCESS;
    }

    let repo_root = find_repo_root();
    let out_dir = out_dir.unwrap_or_else(|| repo_root.join(DEFAULT_OUT_DIR));

    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("failed to create out dir {}: {e}", out_dir.display());
        return ExitCode::from(1);
    }

    let categories = all_categories();
    let all_schemas = schema();
    let mut written = 0usize;

    // Per-category factory installers.
    let factories_dir = out_dir.join("factories");
    for cat in &categories {
        let path = factories_dir.join(format!("{}.ts", cat.name));
        let content = render_category(cat);
        match write_if_changed(&path, &content) {
            Ok(w) => {
                if w.changed {
                    written += 1;
                }
            }
            Err(e) => {
                eprintln!("failed to write {}: {e}", path.display());
                return ExitCode::from(1);
            }
        }
    }

    // factories/index.ts
    {
        let path = factories_dir.join("index.ts");
        let content = render_index(&categories);
        match write_if_changed(&path, &content) {
            Ok(w) => {
                if w.changed {
                    written += 1;
                }
            }
            Err(e) => {
                eprintln!("failed to write {}: {e}", path.display());
                return ExitCode::from(1);
            }
        }
    }

    // reservedOutputNames.ts
    {
        let path = out_dir.join("reservedOutputNames.ts");
        let content = reserved_names_renderer::render();
        match write_if_changed(&path, &content) {
            Ok(w) => {
                if w.changed {
                    written += 1;
                }
            }
            Err(e) => {
                eprintln!("failed to write {}: {e}", path.display());
                return ExitCode::from(1);
            }
        }
    }

    // factoryMetadata.json
    {
        let path = out_dir.join("factoryMetadata.json");
        let value = metadata_renderer::render(&all_schemas);
        let content = serde_json::to_string_pretty(&value)
            .map(|s| format!("{s}\n"))
            .unwrap_or_default();
        match write_if_changed(&path, &content) {
            Ok(w) => {
                if w.changed {
                    written += 1;
                }
            }
            Err(e) => {
                eprintln!("failed to write {}: {e}", path.display());
                return ExitCode::from(1);
            }
        }
    }

    // Top-level index.ts barrel.
    {
        let path = out_dir.join("index.ts");
        let content = "// AUTO-GENERATED — DO NOT EDIT. Run `yarn generate-lib` to regenerate.\n\n\
                       export * from './factories';\n\
                       export * from './reservedOutputNames';\n";
        match write_if_changed(&path, content) {
            Ok(w) => {
                if w.changed {
                    written += 1;
                }
            }
            Err(e) => {
                eprintln!("failed to write {}: {e}", path.display());
                return ExitCode::from(1);
            }
        }
    }

    println!(
        "generate-dsl: wrote/updated {written} files under {}",
        out_dir.display()
    );

    if !no_format {
        if let Err(e) = run_prettier(&out_dir, &repo_root) {
            eprintln!("prettier: {e}");
        }
    }

    ExitCode::SUCCESS
}

/// Walk up from `CARGO_MANIFEST_DIR` until we find a directory containing `package.json`.
fn find_repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut current: &Path = manifest_dir.as_path();
    loop {
        if current.join("package.json").exists() {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(p) => current = p,
            None => return manifest_dir.clone(),
        }
    }
}

fn print_help() {
    eprintln!(
        "Usage: generate-dsl [OPTIONS]\n\n\
         Options:\n  \
           --out-dir <path>        Override output directory (default: {DEFAULT_OUT_DIR})\n  \
           --no-format             Skip prettier formatting step\n  \
           --print-resolver-debug  Print TS type expressions for the first few modules\n  \
           --help, -h              Show this help"
    );
}
