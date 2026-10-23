import { app } from 'electron';
import electronSquirrelStartup from 'electron-squirrel-startup';
import {
    CPU_PROBE_SWITCH,
    ensureCpuSupported,
    failStartup,
    runCpuProbe,
} from './cpuGate';

// `./app` imports the engine addon, which can crash on load on a CPU without
// the extensions it was compiled for. It is loaded only after the check
// passes, and never for Squirrel install/update/uninstall events (which
// create and remove Windows shortcuts), so those work on any CPU. The import
// resolves in a microtask, before `ready`, so `./app` still runs its
// pre-ready setup in time.
if (app.commandLine.hasSwitch(CPU_PROBE_SWITCH)) {
    runCpuProbe();
} else if (electronSquirrelStartup) {
    app.quit();
} else if (ensureCpuSupported()) {
    import('./app').catch((err: unknown) =>
        failStartup(
            'Operator failed to start',
            err instanceof Error ? (err.stack ?? err.message) : String(err),
        ),
    );
}
