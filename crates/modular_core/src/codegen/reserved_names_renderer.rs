//! Render `generated/reservedOutputNames.ts`.
//!
//! Single source of truth: `crates/reserved_output_names.rs`. The runtime no
//! longer needs to fetch via NAPI — the codegen embeds the list at build time.

const HEADER: &str = "// AUTO-GENERATED — DO NOT EDIT.\n// Run `yarn generate-lib` to regenerate.\n";

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reserved_output_names.rs"
));

pub fn render() -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    out.push_str("export const RESERVED_OUTPUT_NAMES = [\n");
    for name in RESERVED_OUTPUT_NAMES.iter() {
        out.push_str(&format!("    {:?},\n", name));
    }
    out.push_str("] as const;\n\n");
    out.push_str("export type ReservedOutputName = (typeof RESERVED_OUTPUT_NAMES)[number];\n\n");
    out.push_str("export const RESERVED_OUTPUT_NAMES_SET: ReadonlySet<string> = new Set(RESERVED_OUTPUT_NAMES);\n");
    out
}
