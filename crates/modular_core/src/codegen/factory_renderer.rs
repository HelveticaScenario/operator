//! Render per-category factory installer files plus the factories/index.ts barrel.
//!
//! The runtime imports `buildAllFactories` and `buildNamespaceTree` from
//! `factories/index.ts`. Each category's `register<Cat>` function adds its
//! modules to a shared `Map<string, FactoryFunction>`. The barrel calls every
//! `register<Cat>` and then converts the flat map into the user-facing nested
//! namespace tree via the existing runtime helper.

use std::fmt::Write;

use crate::types::ModuleSchema;
use super::category::Category;

const HEADER: &str = "// AUTO-GENERATED — DO NOT EDIT.\n// Run `yarn generate-lib` to regenerate.\n";

/// Render `factories/<category>.ts` exporting `register<Cat>(builder, schemas, factories)`.
pub fn render_category(category: &Category) -> String {
    let pascal = pascal_case(category.name);
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    writeln!(
        out,
        "import type {{ ModuleSchema }} from '@modular/core';"
    )
    .unwrap();
    writeln!(out, "import type {{ GraphBuilder }} from '../../runtime/graph';").unwrap();
    writeln!(
        out,
        "import type {{ FactoryFunction }} from '../../runtime/factory/namespaceTree';"
    )
    .unwrap();
    writeln!(
        out,
        "import {{ createFactoryFromName }} from '../../runtime/factory/createFactoryFromName';"
    )
    .unwrap();
    out.push('\n');
    writeln!(
        out,
        "/** Register all `{}` modules into `factories`. */",
        category.name
    )
    .unwrap();
    writeln!(
        out,
        "export function register{pascal}(builder: GraphBuilder, schemas: ModuleSchema[], factories: Map<string, FactoryFunction>): void {{"
    )
    .unwrap();
    for schema in &category.schemas {
        writeln!(
            out,
            "    factories.set({:?}, createFactoryFromName(builder, schemas, {:?}));",
            schema.name, schema.name
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    out
}

/// Render `factories/index.ts`. Delegates the namespace tree assembly back to
/// the hand-written `buildNamespaceTree` so that nesting/sanitization logic
/// stays in one place.
pub fn render_index(categories: &[Category]) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    writeln!(
        out,
        "import type {{ ModuleSchema }} from '@modular/core';"
    )
    .unwrap();
    writeln!(out, "import type {{ GraphBuilder }} from '../../runtime/graph';").unwrap();
    writeln!(
        out,
        "import {{ buildNamespaceTree as buildNamespaceTreeFromFactories }} from '../../runtime/factory/namespaceTree';"
    )
    .unwrap();
    writeln!(
        out,
        "import type {{ FactoryFunction, NamespaceTree }} from '../../runtime/factory/namespaceTree';"
    )
    .unwrap();
    out.push('\n');

    for cat in categories {
        let pascal = pascal_case(cat.name);
        writeln!(
            out,
            "import {{ register{pascal} }} from './{}';",
            cat.name
        )
        .unwrap();
    }
    out.push('\n');

    writeln!(
        out,
        "/** Register every category's factories into a flat name → factory map. */"
    )
    .unwrap();
    writeln!(
        out,
        "export function buildAllFactories(builder: GraphBuilder, schemas: ModuleSchema[]): Map<string, FactoryFunction> {{"
    )
    .unwrap();
    writeln!(out, "    const factories = new Map<string, FactoryFunction>();").unwrap();
    for cat in categories {
        let pascal = pascal_case(cat.name);
        writeln!(out, "    register{pascal}(builder, schemas, factories);").unwrap();
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
    writeln!(
        out,
        "    const flatMap: Record<string, FactoryFunction> = {{}};"
    )
    .unwrap();
    writeln!(out, "    for (const [name, fn] of factories) {{").unwrap();
    writeln!(
        out,
        "        flatMap[sanitizeIdentifier(name)] = fn;"
    )
    .unwrap();
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

fn pascal_case(s: &str) -> String {
    let mut out = String::new();
    let mut up_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            up_next = true;
        } else if up_next {
            out.extend(c.to_uppercase());
            up_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Render a list of all module schemas in a category as `{ name, positionalArgs, outputs }`.
/// Used by the metadata renderer.
pub fn schema_metadata_summary(schema: &ModuleSchema) -> serde_json::Value {
    serde_json::json!({
        "name": schema.name,
        "positionalArgs": schema.positional_args.iter().map(|p| &p.name).collect::<Vec<_>>(),
        "outputs": schema.outputs.iter().map(|o| serde_json::json!({
            "name": o.name,
            "polyphonic": o.polyphonic,
            "default": o.default,
        })).collect::<Vec<_>>(),
    })
}
