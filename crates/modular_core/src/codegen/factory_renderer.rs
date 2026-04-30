//! Render the single `factories.ts` file that drives DSL factory construction.
//!
//! Iterates over the flat `dsp::schema()` list. No category split — categories
//! aren't part of the schema, so maintaining a parallel hardcoded list would be
//! fragile.
//!
//! The generated module exports `buildAllFactories(builder, schemas)` and
//! `buildNamespaceTree(builder, schemas)`. Each `factories.set(name, ...)` line
//! resolves the named schema and binds it to the builder via the hand-written
//! `createFactoryFromName` helper, which delegates to `createFactory`.

use std::fmt::Write;

use crate::types::ModuleSchema;

const HEADER: &str = "// AUTO-GENERATED — DO NOT EDIT.\n// Run `yarn generate-lib` to regenerate.\n";

/// Render `generated/factories.ts`.
pub fn render(schemas: &[ModuleSchema]) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    writeln!(out, "import type {{ ModuleSchema }} from '@modular/core';").unwrap();
    writeln!(out, "import type {{ GraphBuilder }} from '../runtime/graph';").unwrap();
    writeln!(
        out,
        "import {{ buildNamespaceTree as buildNamespaceTreeFromFactories }} from '../runtime/factory/namespaceTree';"
    )
    .unwrap();
    writeln!(
        out,
        "import type {{ FactoryFunction, NamespaceTree }} from '../runtime/factory/namespaceTree';"
    )
    .unwrap();
    writeln!(
        out,
        "import {{ createFactoryFromName }} from '../runtime/factory/createFactoryFromName';"
    )
    .unwrap();
    out.push('\n');

    writeln!(
        out,
        "/** Register every schema's factory into a flat name → factory map. */"
    )
    .unwrap();
    writeln!(
        out,
        "export function buildAllFactories(builder: GraphBuilder, schemas: ModuleSchema[]): Map<string, FactoryFunction> {{"
    )
    .unwrap();
    writeln!(out, "    const factories = new Map<string, FactoryFunction>();").unwrap();
    // Sort by module name so regenerations produce stable diffs even if
    // `dsp::schema()` reorders entries.
    let mut sorted: Vec<&ModuleSchema> = schemas.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for schema in &sorted {
        writeln!(
            out,
            "    factories.set({:?}, createFactoryFromName(builder, schemas, {:?}));",
            schema.name, schema.name
        )
        .unwrap();
    }
    writeln!(out, "    return factories;").unwrap();
    writeln!(out, "}}").unwrap();
    out.push('\n');

    writeln!(
        out,
        "/** Build the user-facing nested DSL namespace tree from the flat factory map. */"
    )
    .unwrap();
    writeln!(
        out,
        "export function buildNamespaceTree(builder: GraphBuilder, schemas: ModuleSchema[]): {{ factories: Map<string, FactoryFunction>; namespaceTree: NamespaceTree }} {{"
    )
    .unwrap();
    writeln!(out, "    const factories = buildAllFactories(builder, schemas);").unwrap();
    writeln!(out, "    const flatMap: Record<string, FactoryFunction> = {{}};").unwrap();
    writeln!(out, "    for (const [name, fn] of factories) {{").unwrap();
    writeln!(out, "        flatMap[sanitizeIdentifier(name)] = fn;").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(
        out,
        "    return {{ factories, namespaceTree: buildNamespaceTreeFromFactories(schemas, flatMap) }};"
    )
    .unwrap();
    writeln!(out, "}}").unwrap();
    out.push('\n');

    // Inline a tiny copy of sanitizeIdentifier matching factory/identifiers.ts.
    out.push_str("function sanitizeIdentifier(name: string): string {\n");
    out.push_str("    let id = name.replace(/[^a-zA-Z0-9_$]+(.)?/g, (_match, chr) => (chr ? chr.toUpperCase() : ''));\n");
    out.push_str("    if (!/^[A-Za-z_$]/.test(id)) id = `_${id}`;\n");
    out.push_str("    return id || '_';\n");
    out.push_str("}\n");

    out
}
