//! `generate-dsl` — write @modular/dsl generated artifacts from `dsp::schema()`.
//!
//! In PR 4 (current) this bin only supports `--print-resolver-debug`, which
//! resolves a small fragment to verify the type resolver works end-to-end.
//! Later PRs add factory/lib emission and `--out-dir`.

use modular_core::codegen::type_resolver::schema_to_type_expr;
use modular_core::dsp::schema;
use std::env;

fn main() {
    let mut print_debug = false;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--print-resolver-debug" => print_debug = true,
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_help();
                std::process::exit(2);
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
        return;
    }

    eprintln!("generate-dsl: no action specified — pass --print-resolver-debug for now");
    std::process::exit(2);
}

fn print_help() {
    eprintln!(
        "Usage: generate-dsl [OPTIONS]\n\n\
         Options:\n  \
           --print-resolver-debug  Print TS type expressions for the first few modules' params schemas\n  \
           --help, -h              Show this help"
    );
}
