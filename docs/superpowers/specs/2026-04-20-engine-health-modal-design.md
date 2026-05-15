# Engine Health Modal — Design Spec

**Date:** 2026-04-20

## Goal

Add a dedicated read-only modal that displays real-time audio engine health metrics (CPU budget usage). Engine health is diagnostic data, not a setting, so it warrants its own modal rather than a tab in the Settings panel.

## Background

The Rust audio engine already collects CPU budget metrics via `AudioBudgetMeter` and exposes them through:
- N-API binding: `Synthesizer.prototype.getHealth() → AudioBudgetSnapshot`
- IPC channel: `SYNTH_GET_HEALTH` (`modular:synth:get-health`)
- Preload bridge: `window.electronAPI.getHealth()`

No UI currently surfaces this data — it is only used in a debug `console.log` and an E2E test helper.

## `AudioBudgetSnapshot` Fields

| Field | Type | Meaning |
|---|---|---|
| `avg_ns_per_sample` | `f64` | Average CPU nanoseconds per sample over the snapshot window |
| `avg_usage` | `f64` | Average real-time CPU usage ratio (1.0 = 100% of RT budget) |
| `peak_ns_per_sample` | `f64` | Worst-case CPU ns/sample in the snapshot window |
| `peak_usage` | `f64` | Worst-case real-time CPU usage ratio |
| `total_samples` | `BigInt` | Cumulative samples processed (since last read) |
| `total_time_ns` | `BigInt` | Cumulative ns spent processing (since last read) |

The snapshot is **reset-on-read**: each `getHealth()` call returns stats for the interval since the last call and resets the accumulators.

## How It Opens

A new **Engine Health...** menu item is added under:
- macOS: **View** menu
- Windows/Linux: **View** menu

This mirrors the pattern used by Settings. No keyboard shortcut is required initially.

The menu item sends a new `OPEN_ENGINE_HEALTH` channel via `mainWindow.webContents.send(...)`. The renderer listens via a new `onMenuOpenEngineHealth` preload bridge method and toggles `isEngineHealthOpen` state in `App.tsx`.

## Component Design

### `EngineHealth.tsx`

- Mirrors `Settings.tsx` structure: overlay -> panel -> header (title + close button) -> body
- Always mounted in the tree (same as `<Settings>`); renders nothing when `isOpen` is false
- On mount (when `isOpen` becomes true): starts a `setInterval(1000)` calling `window.electronAPI.getHealth()`
- On unmount / `isOpen` goes false: clears the interval
- Stores latest `AudioBudgetSnapshot | null` in `useState`
- Shows a loading placeholder until first data arrives

### Display Layout

```
Engine Health
------------------------------
Audio CPU

  Average    12.3%   143 ns/sample
  Peak       45.1%   541 ns/sample
------------------------------
```

Usage percentage color thresholds (applied to avg_usage and peak_usage):
- < 50%  => default text color (--text-primary)
- 50-80% => warning color (--text-warning / orange)
- > 80%  => error color (--text-error / red)

### `EngineHealth.css`

Reuses the same CSS variable tokens as `Settings.css`. Structural class names are prefixed with `engine-health-` to avoid collision. Panel width ~360px.

## Files Changed

| File | Change |
|---|---|
| `src/shared/ipcTypes.ts` | Add `OPEN_ENGINE_HEALTH` to `MENU_CHANNELS` |
| `src/main/main.ts` | Add View menu item (macOS + Win/Linux), send `OPEN_ENGINE_HEALTH` |
| `src/preload/preload.ts` | Expose `onMenuOpenEngineHealth` via `menuEventHandler` |
| `src/renderer/electronAPI.ts` | Add `onMenuOpenEngineHealth` to the API type/export |
| `src/renderer/App.tsx` | Add `isEngineHealthOpen` state, wire listener, mount `<EngineHealth>` |
| `src/renderer/components/EngineHealth.tsx` | New component |
| `src/renderer/components/EngineHealth.css` | New styles |

## Out of Scope

- History/sparkline charts
- Xrun / buffer underrun counters (not yet collected by the engine)
- Keyboard shortcut to open
- Toolbar button
