//! Bundle the DSL TypeScript into a single JS file for the deno_core runtime.
//!
//! Delegates to `apps/operator-zed/dsl/build.mjs`, which uses esbuild's API
//! (the CLI's `--alias` flag can't redirect relative imports). The bundle
//! lands at `$OUT_DIR/dsl_runtime.js` and is `include_str!`-ed by
//! `dsl_runtime.rs`. If `node` or `node_modules` isn't available, a
//! placeholder bundle is written so the binary still builds.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=dsl/entry.ts");
    println!("cargo:rerun-if-changed=dsl/build.mjs");
    println!("cargo:rerun-if-changed=dsl/modular_core_shim.ts");
    println!("cargo:rerun-if-changed=dsl/analyze_source_stub.ts");

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("apps/operator-zed/Cargo.toml has a parent monorepo")
        .to_path_buf();

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let bundle_path = out_dir.join("dsl_runtime.js");

    // Re-bundle whenever DSL source changes upstream.
    println!(
        "cargo:rerun-if-changed={}/src/main/dsl",
        workspace_root.display()
    );
    println!(
        "cargo:rerun-if-changed={}/src/shared/dsl",
        workspace_root.display()
    );

    let node_modules = workspace_root.join("node_modules");
    let build_script = manifest_dir.join("dsl/build.mjs");
    if !node_modules.exists() || !build_script.exists() {
        eprintln!(
            "operator-zed/build.rs: missing {} or {} — writing placeholder bundle",
            node_modules.display(),
            build_script.display()
        );
        write_placeholder_bundle(&bundle_path);
        return;
    }

    let status = Command::new("node")
        .arg(&build_script)
        .arg(&bundle_path)
        .current_dir(&workspace_root)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("operator-zed/build.rs: dsl/build.mjs exited with {s}");
            write_placeholder_bundle(&bundle_path);
        }
        Err(err) => {
            eprintln!("operator-zed/build.rs: failed to spawn node: {err}");
            write_placeholder_bundle(&bundle_path);
        }
    }
}

fn write_placeholder_bundle(path: &Path) {
    let placeholder = r#"// Placeholder — esbuild bundle was not produced at build time.
// Run `yarn install` in the monorepo and rebuild to get the real bundle.
globalThis.modz_executePatchScript = function modz_executePatchScript() {
    return {
        ok: false,
        error: "DSL bundle missing: build.rs could not run dsl/build.mjs. Run `yarn install` and rebuild.",
    };
};
"#;
    std::fs::write(path, placeholder).expect("write placeholder bundle");
}
