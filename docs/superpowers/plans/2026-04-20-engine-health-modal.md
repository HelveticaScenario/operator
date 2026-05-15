# Engine Health Modal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a read-only Engine Health modal that displays real-time audio CPU budget stats (avg/peak usage %, ns/sample), opened from the View menu.

**Architecture:** A new `EngineHealth` React component mirrors the Settings modal structure (overlay + panel + header + body), polls `window.electronAPI.getHealth()` every second while open, and is wired to the renderer via a new `OPEN_ENGINE_HEALTH` menu channel. No new IPC handlers are needed — `SYNTH_GET_HEALTH` already exists.

**Tech Stack:** TypeScript, React, Electron IPC (renderer ← main push via `webContents.send`), existing N-API `getHealth()` binding.

---

### Task 1: Add `OPEN_ENGINE_HEALTH` to `MENU_CHANNELS`

**Files:**
- Modify: `src/shared/ipcTypes.ts:324-334`

- [ ] **Step 1: Add the channel constant**

In `src/shared/ipcTypes.ts`, add `OPEN_ENGINE_HEALTH` to the `MENU_CHANNELS` object (alphabetical order puts it between `NEW_FILE` and `OPEN_SETTINGS`):

```typescript
export const MENU_CHANNELS = {
    CLOSE_BUFFER: 'modular:menu:close-buffer',
    NEW_FILE: 'modular:menu:new-file',
    OPEN_ENGINE_HEALTH: 'modular:menu:open-engine-health',
    OPEN_SETTINGS: 'modular:menu:open-settings',
    OPEN_WORKSPACE: 'modular:menu:open-workspace',
    SAVE: 'modular:menu:save',
    STOP: 'modular:menu:stop',
    TOGGLE_RECORDING: 'modular:menu:toggle-recording',
    UPDATE_PATCH: 'modular:menu:update-patch',
    UPDATE_PATCH_NEXT_BEAT: 'modular:menu:update-patch-next-beat',
} as const;
```

- [ ] **Step 2: Typecheck**

```bash
yarn typecheck
```
Expected: no new errors.

- [ ] **Step 3: Commit**

```bash
git add src/shared/ipcTypes.ts
git commit -m "feat: add OPEN_ENGINE_HEALTH menu channel constant"
```

---

### Task 2: Wire the menu item in the main process

**Files:**
- Modify: `src/main/main.ts:1739-1742`

The View menu is currently `{ role: 'viewMenu' }` (Electron built-in). Replace it with an explicit submenu that includes Electron's default View items plus the Engine Health item.

- [ ] **Step 1: Replace the View menu entry**

In `src/main/main.ts`, replace lines 1739–1742:

```typescript
        // View menu
        {
            role: 'viewMenu',
        },
```

with:

```typescript
        // View menu
        {
            label: 'View',
            submenu: [
                { role: 'reload' },
                { role: 'forceReload' },
                { role: 'toggleDevTools' },
                { type: 'separator' },
                { role: 'resetZoom' },
                { role: 'zoomIn' },
                { role: 'zoomOut' },
                { type: 'separator' },
                { role: 'togglefullscreen' },
                { type: 'separator' },
                {
                    click: () => {
                        if (mainWindow && !mainWindow.isDestroyed()) {
                            mainWindow.webContents.send(MENU_CHANNELS.OPEN_ENGINE_HEALTH);
                        }
                    },
                    label: 'Engine Health...',
                },
            ],
        },
```

- [ ] **Step 2: Typecheck**

```bash
yarn typecheck
```
Expected: no new errors.

- [ ] **Step 3: Commit**

```bash
git add src/main/main.ts
git commit -m "feat: add Engine Health menu item to View menu"
```

---

### Task 3: Expose the menu event in the preload bridge

**Files:**
- Modify: `src/preload/preload.ts`

Two changes are needed: add the type to the `ElectronAPI` interface, and add the implementation in the `electronAPI` object.

- [ ] **Step 1: Add to `ElectronAPI` interface**

In `src/preload/preload.ts`, find the `onMenuOpenSettings` line in the `ElectronAPI` interface (around line 187) and add the new entry after it:

```typescript
    onMenuOpenSettings: (callback: () => void) => () => void;
    onMenuOpenEngineHealth: (callback: () => void) => () => void;
```

- [ ] **Step 2: Add to `electronAPI` object**

Find the `onMenuOpenSettings: menuEventHandler(MENU_CHANNELS.OPEN_SETTINGS),` line (around line 405) and add after it:

```typescript
    onMenuOpenSettings: menuEventHandler(MENU_CHANNELS.OPEN_SETTINGS),
    onMenuOpenEngineHealth: menuEventHandler(MENU_CHANNELS.OPEN_ENGINE_HEALTH),
```

- [ ] **Step 3: Typecheck**

```bash
yarn typecheck
```
Expected: no new errors.

- [ ] **Step 4: Commit**

```bash
git add src/preload/preload.ts
git commit -m "feat: expose onMenuOpenEngineHealth in preload bridge"
```

---

### Task 4: Create `EngineHealth.css`

**Files:**
- Create: `src/renderer/components/EngineHealth.css`

- [ ] **Step 1: Create the CSS file**

Create `src/renderer/components/EngineHealth.css` with the following content:

```css
.engine-health-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
}

.engine-health-panel {
    background: var(--bg-primary);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 360px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    outline: none;
}

.engine-health-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-default);
}

.engine-health-header h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
}

.engine-health-close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 20px;
    line-height: 1;
    padding: 0 4px;
}

.engine-health-close-btn:hover {
    color: var(--text-primary);
}

.engine-health-body {
    padding: 20px;
    overflow-y: auto;
    flex: 1;
}

.engine-health-section {
    margin-bottom: 8px;
}

.engine-health-section-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    margin: 0 0 12px 0;
}

.engine-health-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 4px 0;
}

.engine-health-label {
    font-size: 13px;
    color: var(--text-muted);
    min-width: 70px;
}

.engine-health-values {
    display: flex;
    align-items: baseline;
    gap: 12px;
}

.engine-health-usage {
    font-size: 14px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    min-width: 52px;
    text-align: right;
    color: var(--text-primary);
}

.engine-health-usage.warning {
    color: #d97706;
}

.engine-health-usage.danger {
    color: #dc2626;
}

.engine-health-ns {
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    min-width: 90px;
    text-align: right;
}

.engine-health-loading {
    font-size: 13px;
    color: var(--text-muted);
    text-align: center;
    padding: 20px 0;
}
```

- [ ] **Step 2: Commit**

```bash
git add src/renderer/components/EngineHealth.css
git commit -m "feat: add EngineHealth modal styles"
```

---

### Task 5: Create `EngineHealth.tsx`

**Files:**
- Create: `src/renderer/components/EngineHealth.tsx`

- [ ] **Step 1: Create the component**

Create `src/renderer/components/EngineHealth.tsx`:

```tsx
import React, { useEffect, useRef, useState } from 'react';
import type { AudioBudgetSnapshot } from '@modular/core';
import electronAPI from '../electronAPI';
import './EngineHealth.css';

interface EngineHealthProps {
    isOpen: boolean;
    onClose: () => void;
}

function usageClass(usage: number): string {
    if (usage >= 0.8) return 'danger';
    if (usage >= 0.5) return 'warning';
    return '';
}

function formatUsage(usage: number): string {
    return `${(usage * 100).toFixed(1)}%`;
}

function formatNs(ns: number): string {
    return `${ns.toFixed(0)} ns/sample`;
}

export function EngineHealth({ isOpen, onClose }: EngineHealthProps) {
    const [snapshot, setSnapshot] = useState<AudioBudgetSnapshot | null>(null);
    const panelRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (!isOpen) {
            setSnapshot(null);
            return;
        }

        let cancelled = false;

        const poll = () => {
            electronAPI.synthesizer
                .getHealth()
                .then((data) => {
                    if (!cancelled) setSnapshot(data);
                })
                .catch(console.error);
        };

        poll();
        const intervalId = setInterval(poll, 1000);

        return () => {
            cancelled = true;
            clearInterval(intervalId);
        };
    }, [isOpen]);

    // Focus panel when opened
    useEffect(() => {
        if (isOpen) {
            requestAnimationFrame(() => {
                panelRef.current?.focus();
            });
        }
    }, [isOpen]);

    // Close on Escape
    useEffect(() => {
        if (!isOpen) return;
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') onClose();
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, onClose]);

    if (!isOpen) return null;

    return (
        <div className="engine-health-overlay" onClick={onClose}>
            <div
                className="engine-health-panel"
                ref={panelRef}
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <div className="engine-health-header">
                    <h2>Engine Health</h2>
                    <button className="engine-health-close-btn" onClick={onClose}>
                        ×
                    </button>
                </div>

                <div className="engine-health-body">
                    {snapshot === null ? (
                        <div className="engine-health-loading">Loading…</div>
                    ) : (
                        <div className="engine-health-section">
                            <p className="engine-health-section-title">Audio CPU</p>
                            <div className="engine-health-row">
                                <span className="engine-health-label">Average</span>
                                <div className="engine-health-values">
                                    <span
                                        className={`engine-health-usage ${usageClass(snapshot.avg_usage)}`}
                                    >
                                        {formatUsage(snapshot.avg_usage)}
                                    </span>
                                    <span className="engine-health-ns">
                                        {formatNs(snapshot.avg_ns_per_sample)}
                                    </span>
                                </div>
                            </div>
                            <div className="engine-health-row">
                                <span className="engine-health-label">Peak</span>
                                <div className="engine-health-values">
                                    <span
                                        className={`engine-health-usage ${usageClass(snapshot.peak_usage)}`}
                                    >
                                        {formatUsage(snapshot.peak_usage)}
                                    </span>
                                    <span className="engine-health-ns">
                                        {formatNs(snapshot.peak_ns_per_sample)}
                                    </span>
                                </div>
                            </div>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}
```

- [ ] **Step 2: Typecheck**

```bash
yarn typecheck
```
Expected: no new errors (the component is not yet mounted so it won't be imported yet).

- [ ] **Step 3: Commit**

```bash
git add src/renderer/components/EngineHealth.tsx
git commit -m "feat: add EngineHealth modal component"
```

---

### Task 6: Mount `EngineHealth` in `App.tsx`

**Files:**
- Modify: `src/renderer/App.tsx`

Three changes: add import, add state, wire listener, mount component.

- [ ] **Step 1: Add import**

At the top of `src/renderer/App.tsx`, alongside other component imports, add:

```typescript
import { EngineHealth } from './components/EngineHealth';
```

- [ ] **Step 2: Add state declaration**

After line 123 (`const [isSettingsOpen, setIsSettingsOpen] = useState(false);`), add:

```typescript
    const [isEngineHealthOpen, setIsEngineHealthOpen] = useState(false);
```

- [ ] **Step 3: Wire the menu listener**

Inside the menu listener `useEffect` (around line 851), after the `cleanupOpenSettings` declaration:

```typescript
    const cleanupOpenSettings = electronAPI.onMenuOpenSettings(() => {
        setIsSettingsOpen(true);
    });
    const cleanupOpenEngineHealth = electronAPI.onMenuOpenEngineHealth(() => {
        setIsEngineHealthOpen(true);
    });
```

And add `cleanupOpenEngineHealth()` to the return cleanup:

```typescript
    return () => {
        cleanupNewFile(); cleanupSave(); cleanupStop(); cleanupUpdate();
        cleanupUpdateNextBeat(); cleanupOpenWorkspace(); cleanupCloseBuffer();
        cleanupToggleRecording(); cleanupOpenSettings(); cleanupOpenEngineHealth();
    };
```

- [ ] **Step 4: Mount the component**

After the `<Settings ... />` component (around line 926), add:

```tsx
                <EngineHealth
                    isOpen={isEngineHealthOpen}
                    onClose={() => setIsEngineHealthOpen(false)}
                />
```

- [ ] **Step 5: Typecheck**

```bash
yarn typecheck
```
Expected: no errors.

- [ ] **Step 6: Run unit tests**

```bash
yarn test:unit
```
Expected: all pass (no unit tests touch this code path).

- [ ] **Step 7: Commit**

```bash
git add src/renderer/App.tsx
git commit -m "feat: mount EngineHealth modal and wire menu listener in App"
```

---

### Task 7: E2E smoke test

**Files:**
- Modify or create: `src/renderer/__tests__/e2e/` (check for existing health/modal tests)

- [ ] **Step 1: Check existing E2E test structure**

```bash
ls src/renderer/__tests__/e2e/
```

Look for an existing file like `settings.spec.ts` to understand the test pattern.

- [ ] **Step 2: Add a smoke test for the Engine Health modal**

In the most appropriate existing E2E spec file (or a new `engineHealth.spec.ts`), add:

```typescript
test('Engine Health modal opens and shows data after audio is running', async ({ page }) => {
    // Trigger via the test API (same pattern as existing health test in App.tsx)
    await page.evaluate(() => (window as any).__TEST_API__.openEngineHealth?.());
    await expect(page.getByText('Engine Health')).toBeVisible();
    await expect(page.getByText('Audio CPU')).toBeVisible();
    await expect(page.getByText('Average')).toBeVisible();
    await expect(page.getByText('Peak')).toBeVisible();
});
```

Note: The `__TEST_API__.openEngineHealth` helper needs to be added to `App.tsx` alongside the existing `window.__TEST_API__` block (search for `__TEST_API__` in `App.tsx` to find it). Add:

```typescript
openEngineHealth: () => setIsEngineHealthOpen(true),
```

- [ ] **Step 3: Run E2E tests**

```bash
yarn test:e2e
```
Expected: new test passes, no regressions.

- [ ] **Step 4: Commit**

```bash
git add src/renderer/App.tsx src/renderer/__tests__/e2e/
git commit -m "test: add E2E smoke test for Engine Health modal"
```
