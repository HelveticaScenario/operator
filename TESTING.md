# Testing Guide

This project is an Electron app (React/TypeScript renderer) + Rust DSP engine with a multi-layer test infrastructure.

## Test Layers

### Layer 1: Rust DSP Tests

- **Command:** `cargo test -p modular_core` or `yarn test:rust`
- **Test file:** `crates/modular_core/tests/dsp_fresh_tests.rs`
- **Covers:** oscillator output (sine, saw, pulse), noise, param validators, schema consistency, constructors, multi-module patch routing via `Patch::from_graph()`
- **When to run:** After Rust DSP changes in `crates/modular_core/src/dsp/`

### Layer 2: Vitest Unit/Integration Tests

- **Command:** `yarn test:unit` (runs `vitest run`)
- **Config:** `vitest.config.ts` at project root
- **Test locations:**
    - `src/main/__tests__/patchSimilarityRemap.test.ts` — patch similarity remapping
    - `src/renderer/__tests__/interpolationMapping.test.ts` — interpolation mapping
    - `src/main/dsl/__tests__/executor.test.ts` — DSL → PatchGraph pipeline (exercises full DSL without Electron)
    - `crates/modular/__test__/napi.test.ts` — N-API integration tests (getSchemas, validatePatchGraph, deriveChannelCount)
- **When to run:** After DSL/factory changes, schema changes, or N-API binding changes

### Layer 3: Playwright E2E Tests

- **Command:** `yarn test:e2e`
- **Config:** `playwright.config.ts`
- **Fixtures:** `e2e/fixtures.ts` — launches Electron with `E2E_TEST=1` env var
- **Test files:**
    - `e2e/smoke.test.ts` — app launch, UI elements, JS error checking
    - `e2e/editor.test.ts` — Monaco editor, button functionality, help window, error display
    - `e2e/tests/dsl-execution.test.ts` — DSL execution via `__TEST_API__`, audio flow verification
    - `e2e/tests/errors.test.ts` — error handling for invalid DSL
    - `e2e/tests/settings.test.ts` — settings panel interaction
    - `e2e/tests/patch-editing.test.ts` — patch modification and re-execution
- **Prerequisites:** `.webpack/main` and `.webpack/renderer` must exist (run `yarn start` once, or `npx electron-forge build`)
- The renderer exposes `window.__TEST_API__` when `E2E_TEST=1`, providing:
    - `getEditorValue()` / `setEditorValue(code)` — interact with Monaco programmatically
    - `executePatch()` — trigger patch evaluation
    - `getLastPatchResult()` — DSL execution result including errors
    - `getScopeData()` — current oscilloscope buffer data
    - `getAudioHealth()` — synth health result
    - `isClockRunning()` — whether audio is processing
- **When to run:** After UI/UX changes, renderer changes

### All Tests

- **Command:** `yarn test:all` (runs unit + rust + e2e)

## Agent Workflow

### After Rust DSP changes:

```bash
cargo test -p modular_core
```

### After DSL/factory changes:

```bash
yarn test:unit
```

### After UI/UX changes:

```bash
yarn test:e2e
```

### Quick Reference

| Change type            | Test command                    |
| ---------------------- | ------------------------------- |
| Rust DSP modules       | `cargo test -p modular_core`    |
| DSL factories/executor | `yarn test:unit`                |
| N-API bindings         | `yarn test:unit` (napi.test.ts) |
| Renderer UI/UX         | `yarn test:e2e`                 |
| Everything             | `yarn test:all`                 |

## Notes

- E2E tests require the webpack build to exist. Run `yarn start` once before running E2E tests.
- Vitest is the JS test runner (replaced AVA). Config is in `vitest.config.ts`.
- `window.__TEST_API__` is only exposed when `E2E_TEST=1` env var is set (handled automatically by the E2E fixtures).
- Rust DSP tests use `Patch::from_graph()` (a `#[cfg(test)]`-only helper) to build multi-module patches for integration testing without the audio thread.
