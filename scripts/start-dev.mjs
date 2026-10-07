#!/usr/bin/env node

import { execFileSync, spawn } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const FORGE_BIN = join(ROOT, 'node_modules', '.bin', 'electron-forge');

// Name the dev Electron.app after this checkout before launching it.
execFileSync(
    process.execPath,
    [join(ROOT, 'scripts', 'patch-electron-plist.mjs')],
    {
        stdio: 'inherit',
    },
);

// Pass wrapper PID so Electron can signal us to restart
process.env.DEV_WRAPPER_PID = String(process.pid);

const child = spawn(FORGE_BIN, ['start'], {
    stdio: ['pipe', 'inherit', 'inherit'],
    cwd: ROOT,
});

// Forward terminal stdin to forge (for manual `rs` command)
process.stdin.pipe(child.stdin, { end: false });
process.stdin.on('error', () => {});

// Listen for restart signal from Electron main process
process.on('SIGUSR1', () => {
    console.log('[dev] Restart requested, sending rs to forge...');
    child.stdin.write('rs\n');
});

child.on('exit', (code) => {
    process.exit(code ?? 0);
});

function cleanup() {
    child.kill('SIGTERM');
    process.exit();
}
process.on('SIGINT', cleanup);
process.on('SIGTERM', cleanup);
