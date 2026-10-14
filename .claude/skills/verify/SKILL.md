---
name: verify
description: Launch and drive the Operator Electron app for runtime verification of renderer/editor changes — build handles, single-instance workaround, Playwright driving recipe.
---

# Verifying Operator at runtime

The app is Electron + Vite (`@electron-forge/plugin-vite`). The built entry is `.vite/build/main.js` (CJS), produced by `electron-forge start`; `e2e/fixtures.ts` launches the same entry. In dev builds it has a Vite dev-server URL baked in (grep `http://localhost:` in `.vite/build/main.js`).

## Launch recipe (Playwright, coexists with a running dev instance)

1. Build once: `npx electron-forge start` (then let it exit or kill it). If another dev instance is running, the launched app quits via the single-instance lock — the build artifacts still land in `.vite/build/`.
2. Serve the renderer on the baked port: `npx vite --config vite.renderer.config.ts --port <baked-port> --strictPort` (index.html at repo root).
3. The single-instance lock is keyed on the userData dir. Launch through a wrapper CJS that redirects it, so a concurrently running dev instance is untouched:
    ```js
    const { app } = require('electron');
    app.setPath('userData', process.env.QF_USER_DATA);
    require('<repo>/.vite/build/main.js');
    ```
4. Playwright: `_electron.launch({ args: [wrapper.cjs], executablePath: '<repo>/node_modules/.bin/electron', cwd: repo, env: { E2E_TEST: '1', E2E_WORKSPACE: tmpDir, QF_USER_DATA: tmpDir2, ELECTRON_DISABLE_GPU: '1', NODE_ENV: 'test' } })`. `main.ts` honors `E2E_WORKSPACE` directly.
5. After `firstWindow()`: `domcontentloaded` → `reload()` → `networkidle` (mirrors e2e/fixtures.ts).

## Driving the editor

- Seed a `patch.js` into the E2E workspace dir before launch; the app opens with **no buffer** — `.monaco-editor` is absent until you click the file in the EXPLORER "workspace files" list (`getByText('patch.js')`).
- Sidebar tab labels are CSS-uppercased — match `getByText(/^control$/i)`, not `'CONTROL'`.
- The patch editor is the Monaco editor whose model language is `'javascript'` (`window.monaco.editor.getEditors().find(...)`); an `inmemory://` plaintext model also exists.
- `window.__TEST_API__` (E2E_TEST=1): `executePatch()` fires the submit flow without awaiting — poll `getLastPatchResult()` until non-null. Also `setEditorValue`, `getScopeData`, `getAudioHealth`.
- Trigger editor actions via `editor.trigger('src', 'editor.action.quickFix', null)` etc.; the action-widget rows are `.action-widget .monaco-list-row`.
- ControlPanel sliders are native `input[type="range"]` — drag with `mouse.down()/move()/up()` on the bounding box; source write-back lands in the Monaco model.

## Gotchas

- Kill only your own instances: `pkill -f <your-wrapper-name>` — the user often has a dev instance from another worktree running.
- The Monaco lightbulb renders on the line **below** the cursor when the current line is cramped; its codicon class varies (`lightbulb-autofix` when an action `isPreferred`), so assert on the action widget, not the bulb icon.
