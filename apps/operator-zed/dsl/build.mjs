// Bundle entry.ts -> $OUT_FILE for the operator-zed deno_core runtime.
//
// Driven by Cargo's build.rs (apps/operator-zed/build.rs). esbuild's CLI
// `--alias` flag matches on the literal module specifier as written, so it
// can't redirect a relative import like `./analyzeSource` from inside
// executor.ts. The plugin below intercepts the resolution by absolute path
// instead, redirecting two files:
//
//   src/main/dsl/analyzeSource.ts -> apps/operator-zed/dsl/analyze_source_stub.ts
//     Skips ts-morph and the ~14 MB TypeScript compiler.
//
//   crates/modular/index.js (via @modular/core) -> apps/operator-zed/dsl/modular_core_shim.ts
//     Skips the N-API addon entry; pure JS replacements for the values
//     the DSL actually uses (deriveChannelCount, getReservedOutputNames).
//
// Usage:
//   node apps/operator-zed/dsl/build.mjs <out_file>

import * as path from 'node:path';
import * as url from 'node:url';
import { build } from 'esbuild';

const here = path.dirname(url.fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(here, '..', '..', '..');

const outFile = process.argv[2];
if (!outFile) {
    console.error('usage: node build.mjs <out_file>');
    process.exit(2);
}

const analyzeSourceTs = path.join(
    workspaceRoot,
    'src',
    'main',
    'dsl',
    'analyzeSource.ts',
);
const analyzeSourceStub = path.join(here, 'analyze_source_stub.ts');
const modularCoreShim = path.join(here, 'modular_core_shim.ts');

const redirectPlugin = {
    name: 'modz-redirect',
    setup(b) {
        b.onResolve({ filter: /.*/ }, (args) => {
            // @modular/core npm package
            if (args.path === '@modular/core') {
                return { path: modularCoreShim };
            }
            return null;
        });
        b.onResolve({ filter: /analyzeSource$/ }, (args) => {
            // Relative `./analyzeSource` from executor.ts (no extension).
            if (!args.path.startsWith('.')) return null;
            const resolved = path.resolve(args.resolveDir, args.path);
            // Match either `<dir>/analyzeSource` or `<dir>/analyzeSource.ts`.
            const candidate1 = resolved + '.ts';
            if (
                candidate1 === analyzeSourceTs ||
                resolved === analyzeSourceTs.replace(/\.ts$/, '')
            ) {
                return { path: analyzeSourceStub };
            }
            return null;
        });
    },
};

await build({
    entryPoints: [path.join(here, 'entry.ts')],
    bundle: true,
    format: 'iife',
    target: 'es2022',
    platform: 'neutral',
    mainFields: ['module', 'main'],
    outfile: outFile,
    plugins: [redirectPlugin],
    // Logging level keeps output reasonable in build.rs stderr.
    logLevel: 'warning',
    // We never load JSON via fs at runtime; this lets the bundle reference
    // any imported JSON literally as data.
    loader: { '.json': 'json' },
});

console.error(`[modz/build] wrote ${outFile}`);
