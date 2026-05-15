#!/usr/bin/env node
// Copy platform-specific debug-info bundles from cargo's `target/` output to
// sit alongside the napi-generated `.node` file so std::backtrace can resolve
// file:line in production panic logs.
//
// macOS:   target/<triple>/release/libmodular.dylib.dSYM
//          → crates/modular/<name>.node.dSYM (inner DWARF renamed)
// Windows: target/<triple>/release/modular.pdb
//          → crates/modular/<name>.node.pdb
// Linux:   no-op — DWARF is embedded in the `.so` by cargo's default
//          `split-debuginfo = "off"`, so no extra file to ship.
import {
    copyFileSync,
    cpSync,
    existsSync,
    readdirSync,
    renameSync,
    rmSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(here);
const crateDir = join(repoRoot, 'crates', 'modular');

const nodeFile = readdirSync(crateDir).find(
    (f) => f.startsWith('operator.') && f.endsWith('.node'),
);
if (!nodeFile) {
    console.warn(
        `[copy-debuginfo] no operator.*.node in ${crateDir}, skipping`,
    );
    process.exit(0);
}

if (process.platform === 'darwin') {
    const arch = process.arch === 'arm64' ? 'aarch64' : 'x86_64';
    const tripleDir = join(
        repoRoot,
        'target',
        `${arch}-apple-darwin`,
        'release',
    );
    const dsymSrc = join(tripleDir, 'libmodular.dylib.dSYM');
    if (!existsSync(dsymSrc)) {
        console.warn(`[copy-debuginfo] no dSYM at ${dsymSrc}, skipping`);
        process.exit(0);
    }

    const dsymDest = join(crateDir, `${nodeFile}.dSYM`);
    if (existsSync(dsymDest)) {
        rmSync(dsymDest, { recursive: true, force: true });
    }
    cpSync(dsymSrc, dsymDest, { recursive: true, dereference: true });

    // std::backtrace (via backtrace-rs) looks for the inner DWARF file at
    // `<binary>.dSYM/Contents/Resources/DWARF/<basename(binary)>`. cargo
    // emits the bundle with the dylib's name (libmodular.dylib); rename it
    // to match the .node basename so co-located lookup succeeds. UUID is
    // preserved by the rename.
    const dwarfDir = join(dsymDest, 'Contents', 'Resources', 'DWARF');
    if (existsSync(dwarfDir)) {
        for (const entry of readdirSync(dwarfDir)) {
            if (entry !== nodeFile) {
                renameSync(join(dwarfDir, entry), join(dwarfDir, nodeFile));
            }
        }
    }
    console.log(`[copy-debuginfo] copied ${dsymSrc} → ${dsymDest}`);
} else if (process.platform === 'win32') {
    const arch = process.arch === 'x64' ? 'x86_64' : process.arch;
    const tripleDir = join(
        repoRoot,
        'target',
        `${arch}-pc-windows-msvc`,
        'release',
    );
    // The MSVC linker writes `<crate>.pdb` next to the .dll. Cargo emits
    // `modular.pdb` for the `modular` crate (named after the dylib stem,
    // dashes replaced with underscores).
    const pdbSrc = join(tripleDir, 'modular.pdb');
    if (!existsSync(pdbSrc)) {
        console.warn(`[copy-debuginfo] no .pdb at ${pdbSrc}, skipping`);
        process.exit(0);
    }
    // dbghelp on Windows finds a .pdb via the path embedded in the PE header
    // (set by the linker at the original build location) then falls back to
    // co-located by binary basename. Co-locate as `<binary>.pdb` so the
    // shipped app works regardless of original build path.
    const pdbDest = join(crateDir, `${nodeFile}.pdb`);
    if (existsSync(pdbDest)) {
        rmSync(pdbDest, { force: true });
    }
    copyFileSync(pdbSrc, pdbDest);
    console.log(`[copy-debuginfo] copied ${pdbSrc} → ${pdbDest}`);
} else {
    // Linux: DWARF is embedded in the .so via cargo's default
    // `split-debuginfo = "off"`. Nothing to copy.
    process.exit(0);
}
