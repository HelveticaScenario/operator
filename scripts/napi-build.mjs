#!/usr/bin/env node
// Runs `napi build` for crates/modular, forwarding all arguments.
//
// Windows locks a DLL while it is loaded, so a running dev instance's
// `operator.*.node` cannot be overwritten by napi's final copy, but it can be
// renamed. The addon is moved aside for the build and moved back if the build
// produces no replacement, so a failed build never leaves the checkout without
// an addon. Set-aside copies are deleted once no process has them loaded.
import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, renameSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const crateDir = join(
    dirname(fileURLToPath(import.meta.url)),
    '..',
    'crates',
    'modular',
);
const STALE_MARKER = '.stale-';

const isAddon = (name) =>
    name.startsWith('operator.') && name.endsWith('.node');
const isSetAside = (name) =>
    name.startsWith('operator.') && name.includes(`.node${STALE_MARKER}`);

function removeSetAsideAddons() {
    for (const name of readdirSync(crateDir).filter(isSetAside)) {
        try {
            rmSync(join(crateDir, name));
        } catch {
            // Still loaded by a running app
        }
    }
}

/** Renames loaded addons aside; returns [original, setAside] path pairs. */
function setAsideAddons() {
    const moved = [];
    for (const name of readdirSync(crateDir).filter(isAddon)) {
        const from = join(crateDir, name);
        const to = `${from}${STALE_MARKER}${Date.now()}`;
        try {
            renameSync(from, to);
            moved.push([from, to]);
        } catch (err) {
            console.warn(
                `[napi-build] could not set aside ${name}: ${err.message}`,
            );
        }
    }
    return moved;
}

const windows = process.platform === 'win32';
let moved = [];
if (windows) {
    removeSetAsideAddons();
    moved = setAsideAddons();
}

const requireFromCrate = createRequire(join(crateDir, 'package.json'));
const cliPkgPath = requireFromCrate.resolve('@napi-rs/cli/package.json');
const cliBin = JSON.parse(readFileSync(cliPkgPath, 'utf8')).bin.napi;
const result = spawnSync(
    process.execPath,
    [join(dirname(cliPkgPath), cliBin), 'build', ...process.argv.slice(2)],
    { stdio: 'inherit', cwd: crateDir },
);

if (windows) {
    const built = new Set(readdirSync(crateDir).filter(isAddon));
    for (const [from, to] of moved) {
        if (!built.has(basename(from))) renameSync(to, from);
    }
    removeSetAsideAddons();
}

process.exit(result.status ?? 1);
