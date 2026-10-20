import { app, dialog } from 'electron';
import { spawnSync, type SpawnSyncReturns } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

/** Command-line switch that makes the app run as the CPU probe child. */
export const CPU_PROBE_SWITCH = 'cpu-probe';

const PROBE_UNSUPPORTED_EXIT = 3;
const PROBE_FAILED_EXIT = 4;
const PROBE_TIMEOUT_MS = 15_000;
/** `STATUS_ILLEGAL_INSTRUCTION`, the exit code of a Windows process killed by one. */
const WINDOWS_ILLEGAL_INSTRUCTION = 0xc000001d;

export type ProbeOutcome = 'supported' | 'unsupported' | 'inconclusive';

/**
 * Only a clean exit or a definite missing-instruction result decides; any
 * other failure is inconclusive and lets the app start rather than blocking
 * a CPU that may be fine.
 */
export function interpretProbe(
    result: Pick<SpawnSyncReturns<Buffer>, 'status' | 'signal' | 'error'>,
): ProbeOutcome {
    if (result.error) {
        return 'inconclusive';
    }
    if (result.status === 0) {
        return 'supported';
    }
    if (
        result.status === PROBE_UNSUPPORTED_EXIT ||
        result.status === WINDOWS_ILLEGAL_INSTRUCTION ||
        result.signal === 'SIGILL'
    ) {
        return 'unsupported';
    }
    return 'inconclusive';
}

/**
 * Probe-child entry point: loads the addon, asks it whether this CPU has the
 * instructions it was compiled for, and exits with the answer. Every error
 * path exits, because an uncaught error in a packaged app opens a dialog and
 * would stall the parent until the probe times out.
 */
export function runCpuProbe(): void {
    import('@modular/core').then(
        (core) => {
            try {
                process.exit(
                    core.cpuSupportsBuild() ? 0 : PROBE_UNSUPPORTED_EXIT,
                );
            } catch {
                process.exit(PROBE_FAILED_EXIT);
            }
        },
        () => process.exit(PROBE_FAILED_EXIT),
    );
}

function probeCacheKey(): string {
    return `${app.getVersion()}|${os.cpus()[0]?.model ?? ''}`;
}

/**
 * Returns whether startup may continue, after showing an error dialog if
 * this CPU cannot run the engine addon. Packaged x86-64 builds for Windows
 * and Linux probe in a child process, since loading the addon on a CPU
 * without its compiled-for extensions crashes with an illegal instruction.
 * A passing result is cached per app version and CPU model.
 *
 * Skipped on macOS, where every Intel Mac that runs Electron's minimum macOS
 * (12) supports x86-64-v2; for Squirrel install/uninstall events, which must
 * work on any CPU; and for unpackaged dev and test launches.
 */
export function ensureCpuSupported(): boolean {
    if (
        !app.isPackaged ||
        process.arch !== 'x64' ||
        process.platform === 'darwin' ||
        process.argv.some((arg) => arg.startsWith('--squirrel-'))
    ) {
        return true;
    }

    const cachePath = path.join(app.getPath('userData'), 'cpu-probe.json');
    const key = probeCacheKey();
    try {
        if (JSON.parse(fs.readFileSync(cachePath, 'utf8')).key === key) {
            return true;
        }
    } catch {
        // No cached result; probe below.
    }

    const outcome = interpretProbe(
        spawnSync(process.execPath, [`--${CPU_PROBE_SWITCH}`], {
            stdio: 'ignore',
            timeout: PROBE_TIMEOUT_MS,
        }),
    );
    if (outcome === 'supported') {
        try {
            fs.mkdirSync(path.dirname(cachePath), { recursive: true });
            fs.writeFileSync(cachePath, JSON.stringify({ key }));
        } catch {
            // Caching is an optimisation; the next launch probes again.
        }
    }
    if (outcome !== 'unsupported') {
        return true;
    }

    dialog.showErrorBox(
        'Unsupported processor',
        `Operator needs a processor with SSE4.2 and POPCNT: Intel Core from 2008 onward, or AMD from 2011 onward.\n\nThis computer's processor (${os.cpus()[0]?.model ?? 'unknown'}) does not support them.`,
    );
    app.exit(1);
    return false;
}
