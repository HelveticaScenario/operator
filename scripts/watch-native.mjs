#!/usr/bin/env node

import { watch } from 'node:fs';
import { spawn } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const CRATES_DIR = join(ROOT, 'crates');
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
        try {
            process.kill(-pid, 'SIGTERM');
        } catch {
            // Process already exited
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

    const child = spawn('yarn', [buildScript], {
        stdio: 'inherit',
        detached: true,
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
