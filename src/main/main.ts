import { app } from 'electron';
import { CPU_PROBE_SWITCH, ensureCpuSupported, runCpuProbe } from './cpuGate';

// `./app` imports the engine addon, which can crash on load on a CPU without
// the extensions it was compiled for, so it is loaded only after the check
// passes. The import resolves in a microtask, before `ready`, so `./app`
// still runs its pre-ready setup in time.
if (app.commandLine.hasSwitch(CPU_PROBE_SWITCH)) {
    runCpuProbe();
} else if (ensureCpuSupported()) {
    void import('./app');
}
