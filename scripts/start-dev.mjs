#!/usr/bin/env node

import { execFileSync, spawn } from 'node:child_process';
import { readFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
// Run the CLI's JS entry with node: the `.bin` shim is a `.cmd` on Windows,
// which `spawn` cannot execute without a shell.
const FORGE_PKG = createRequire(import.meta.url).resolve(
    '@electron-forge/cli/package.json',
);
const FORGE_CLI = join(
    dirname(FORGE_PKG),
    JSON.parse(readFileSync(FORGE_PKG, 'utf8')).bin['electron-forge'],
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
// main-process restart.
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

// A Unix socket file outlives a wrapper that was killed outright, and its path
// repeats whenever the PID is reused.
function removeSocket() {
    if (process.platform !== 'win32') {
        rmSync(RESTART_SOCKET, { force: true });
    }
}
removeSocket();
restartServer.listen(RESTART_SOCKET);

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
