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

/** `/proc/cpuinfo` flags for the x86-64-v2 extensions the addon is compiled to assume. */
const LINUX_REQUIRED_FLAGS = [
    'pni',
    'ssse3',
    'cx16',
    'sse4_1',
    'sse4_2',
    'popcnt',
    'lahf_lm',
];

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

/** Reads the first CPU's feature flags from `/proc/cpuinfo` text. */
export function cpuinfoOutcome(cpuinfo: string): ProbeOutcome {
    const flags = /^flags\s*:(.*)$/m.exec(cpuinfo)?.[1].trim().split(/\s+/);
    if (!flags) {
        return 'inconclusive';
    }
    return LINUX_REQUIRED_FLAGS.every((flag) => flags.includes(flag))
        ? 'supported'
        : 'unsupported';
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

/**
 * Windows has no file listing CPU features, so the app runs itself as a probe
 * child that loads the addon. Any decisive or inconclusive result is cached
 * per app version and CPU model, so the probe runs once per update. The cache
 * lives in the temp dir, outside every profile.
 */
function windowsProbeOutcome(): ProbeOutcome {
    const cachePath = path.join(os.tmpdir(), 'operator-cpu-probe.json');
    const key = `${app.getVersion()}|${os.cpus()[0]?.model ?? ''}`;
    try {
        const cached = JSON.parse(fs.readFileSync(cachePath, 'utf8'));
        if (cached.key === key && cached.outcome !== 'unsupported') {
            return cached.outcome;
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
    try {
        fs.writeFileSync(cachePath, JSON.stringify({ key, outcome }));
    } catch {
        // Caching is an optimisation; the next launch probes again.
    }
    return outcome;
}

/**
 * Shows an error dialog and quits. Waits for `ready`, because before it Linux
 * prints the dialog to stderr instead of showing it.
 */
export function failStartup(title: string, message: string): void {
    void app.whenReady().then(() => {
        dialog.showErrorBox(title, message);
        app.exit(1);
    });
}

/**
 * Returns whether startup may continue. On a packaged x86-64 Windows or Linux
 * build whose CPU lacks the extensions the engine addon was compiled for, it
 * shows an error and quits instead, since loading the addon there crashes
 * with an illegal instruction.
 *
 * Skipped on macOS, where every Intel Mac that runs Electron's minimum macOS
 * (12) supports x86-64-v2, and for unpackaged dev and test launches.
 */
export function ensureCpuSupported(): boolean {
    if (!app.isPackaged || process.arch !== 'x64') {
        return true;
    }

    let outcome: ProbeOutcome;
    if (process.platform === 'linux') {
        try {
            outcome = cpuinfoOutcome(fs.readFileSync('/proc/cpuinfo', 'utf8'));
        } catch {
            outcome = 'inconclusive';
        }
    } else if (process.platform === 'win32') {
        outcome = windowsProbeOutcome();
    } else {
        return true;
    }
    if (outcome !== 'unsupported') {
        return true;
    }

    failStartup(
        'Unsupported processor',
        `Operator needs a processor with SSE4.2 and POPCNT: Intel Core from 2008 onward, or AMD from 2011 onward.\n\nThis computer's processor (${os.cpus()[0]?.model ?? 'unknown'}) does not support them.`,
    );
    return false;
}
