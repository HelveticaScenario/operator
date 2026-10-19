#!/usr/bin/env node

import { execFileSync, spawn } from 'node:child_process';
import { rmSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
// Run the CLI's JS entry with node: the `.bin` shim is a `.cmd` on Windows,
// which `spawn` cannot execute without a shell.
const FORGE_CLI = join(
    ROOT,
    'node_modules',
    '@electron-forge',
    'cli',
    'dist',
    'electron-forge.js',
);

// Name the dev Electron.app after this checkout before launching it.
execFileSync(
    process.execPath,
    [join(ROOT, 'scripts', 'patch-electron-plist.mjs')],
    {
        stdio: 'inherit',
    },
);

// Electron connects to this socket (a named pipe on Windows) to ask for a
// main-process restart; signals cannot carry that request on Windows.
const RESTART_SOCKET =
    process.platform === 'win32'
        ? `\\\\.\\pipe\\operator-dev-restart-${process.pid}`
        : join(tmpdir(), `operator-dev-restart-${process.pid}.sock`);
process.env.DEV_RESTART_SOCKET = RESTART_SOCKET;

const child = spawn(process.execPath, [FORGE_CLI, 'start'], {
    stdio: ['pipe', 'inherit', 'inherit'],
    cwd: ROOT,
});

// Forward terminal stdin to forge (for manual `rs` command)
process.stdin.pipe(child.stdin, { end: false });
process.stdin.on('error', () => {});

const restartServer = createServer((socket) => {
    socket.resume();
    socket.on('error', () => {});
    console.log('[dev] Restart requested, sending rs to forge...');
    child.stdin.write('rs\n');
});
restartServer.on('error', (err) => {
    console.warn(`[dev] Restart socket unavailable: ${err.message}`);
});
restartServer.listen(RESTART_SOCKET);

function removeSocket() {
    if (process.platform !== 'win32') {
        rmSync(RESTART_SOCKET, { force: true });
    }
}

child.on('exit', (code) => {
    removeSocket();
    process.exit(code ?? 0);
});

function cleanup() {
    child.kill('SIGTERM');
    removeSocket();
    process.exit();
}
process.on('SIGINT', cleanup);
process.on('SIGTERM', cleanup);
