#!/usr/bin/env node

import { readdirSync, renameSync, rmSync, watch } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const CRATES_DIR = join(ROOT, 'crates');
const NATIVE_DIR = join(CRATES_DIR, 'modular');
const DEBOUNCE_MS = 500;

// `yarn watch-native-alloc` passes --features=alloc-detector so rebuilds keep the
// dev-only allocation detector compiled in; plain `watch-native` builds without it.
const allocMode = process.argv.includes('--features=alloc-detector');
const buildScript = allocMode ? 'build-native-alloc' : 'build-native';

let buildProcess = null;
let debounceTimer = null;

function killBuild() {
    if (buildProcess) {
        const pid = buildProcess.pid;
        buildProcess = null;
        if (process.platform === 'win32') {
            // Windows has no process groups; taskkill /T ends the build's tree.
            spawnSync('taskkill', ['/pid', String(pid), '/T', '/F'], {
                stdio: 'ignore',
            });
            return;
        }
        try {
            process.kill(-pid, 'SIGTERM');
        } catch {
            // Process already exited
        }
    }
}

// Windows locks a DLL while it is loaded, so the running app's `.node` cannot
// be overwritten by the build, but it can be renamed. Move it aside and delete
// earlier set-aside copies that are no longer loaded.
function releaseLoadedAddon() {
    if (process.platform !== 'win32') return;
    for (const name of readdirSync(NATIVE_DIR)) {
        const path = join(NATIVE_DIR, name);
        if (name.includes('.node.stale-')) {
            try {
                rmSync(path);
            } catch {
                // Still loaded by a running app
            }
        } else if (name.endsWith('.node')) {
            renameSync(path, `${path}.stale-${Date.now()}`);
        }
    }
}

function startBuild() {
    const wasCancelled = buildProcess !== null;
    killBuild();

    if (wasCancelled) {
        console.log('[watch] Cancelled in-progress build');
    }
    console.log('[watch] Rebuilding native module...');
    releaseLoadedAddon();

    // `yarn` is a `.cmd` shim on Windows, which only runs through a shell.
    const isWindows = process.platform === 'win32';
    const child = spawn('yarn', [buildScript], {
        stdio: 'inherit',
        detached: !isWindows,
        shell: isWindows,
        cwd: ROOT,
    });

    buildProcess = child;

    child.on('close', (code, signal) => {
        if (buildProcess !== child) return; // Was replaced by a newer build
        buildProcess = null;
        if (code === 0) {
            console.log('[watch] Build succeeded');
        } else if (signal) {
            console.log(`[watch] Build killed (${signal})`);
        } else {
            console.log(`[watch] Build failed (exit ${code})`);
        }
    });
}

function onChange(_eventType, filename) {
    if (!filename) return;
    if (!filename.endsWith('.rs') && !filename.endsWith('Cargo.toml')) return;

    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
        console.log(`[watch] Change detected: ${filename}`);
        startBuild();
    }, DEBOUNCE_MS);
}

// Clean up child processes on exit
function cleanup() {
    killBuild();
    process.exit();
}

process.on('SIGINT', cleanup);
process.on('SIGTERM', cleanup);

watch(CRATES_DIR, { recursive: true }, onChange);
console.log(
    `[watch] Watching crates/ for changes...${allocMode ? ' (alloc-detector enabled)' : ''}`,
);
