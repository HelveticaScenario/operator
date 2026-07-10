/**
 * Shared Playwright fixtures for Electron E2E tests.
 *
 * Provides `electronApp` and `window` fixtures that launch the app once per
 * test file and expose the first BrowserWindow as a Playwright Page.
 *
 * Requirements:
 *   - The Vite main/preload bundles must exist (.vite/build). Run `yarn start`
 *     once before running E2E.
 *   - Set E2E_TEST=1 env var so the renderer exposes window.__TEST_API__.
 */

import { test as base, type Page } from '@playwright/test';
import { _electron as electron, type ElectronApplication } from 'playwright';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

// Resolve paths relative to the project root
const projectRoot = path.resolve(__dirname, '..');
const electronBin = path.join(projectRoot, 'node_modules', '.bin', 'electron');
const mainEntry = path.join(projectRoot, '.vite', 'build', 'main.js');

export type TestFixtures = {
    electronApp: ElectronApplication;
    window: Page;
};

/**
 * Extended Playwright test with `electronApp` and `window` fixtures.
 *
 * Usage:
 *   import { test, expect } from './fixtures';
 *   test('my test', async ({ window }) => { ... });
 */
export const test = base.extend<TestFixtures>({
    electronApp: async ({}, use) => {
        // Create a temp workspace directory so the app doesn't show the
        // "Open Folder" empty-state screen during tests.
        const tmpWorkspace = fs.mkdtempSync(
            path.join(os.tmpdir(), 'modular-e2e-'),
        );
        // Isolated userData: the app holds a single-instance lock there, so
        // sharing the default dir with a running dev session would make the
        // test instance exit immediately (and tests would inherit the dev
        // session's config).
        const tmpUserData = fs.mkdtempSync(
            path.join(os.tmpdir(), 'modular-e2e-data-'),
        );

        const app = await electron.launch({
            args: [mainEntry, `--user-data-dir=${tmpUserData}`],
            executablePath: electronBin,
            env: {
                ...process.env,
                E2E_TEST: '1',
                E2E_WORKSPACE: tmpWorkspace,
                // Disable hardware acceleration for CI/headless stability
                ELECTRON_DISABLE_GPU: '1',
                // Prevent the app from trying to restore window positions
                NODE_ENV: 'test',
            },
        });

        // Override the workspace IPC handler in the main process so the
        // renderer sees an open workspace (avoids the "Open Folder" screen).
        // This works even against an already-built main bundle.
        await app.evaluate(({ ipcMain }, workspace) => {
            ipcMain.removeHandler('modular:fs:get-workspace');
            ipcMain.handle('modular:fs:get-workspace', () => ({
                path: workspace,
            }));

            ipcMain.removeHandler('modular:fs:list-files');
            ipcMain.handle('modular:fs:list-files', () => []);
        }, tmpWorkspace);

        await use(app);
        await app.close();

        // Clean up the temp workspace
        fs.rmSync(tmpWorkspace, { recursive: true, force: true });
        fs.rmSync(tmpUserData, { recursive: true, force: true });
    },

    window: async ({ electronApp }, use) => {
        const window = await electronApp.firstWindow();
        // Wait for the renderer to be fully loaded
        await window.waitForLoadState('domcontentloaded');

        // Reload the page so the renderer picks up the overridden workspace
        // IPC handler (the initial load may have already queried before the
        // override was installed).
        await window.reload();
        await window.waitForLoadState('domcontentloaded');
        // Give React time to mount and render the UI
        await window.waitForLoadState('networkidle');
        await use(window);
    },
});

export { expect } from '@playwright/test';

/**
 * Open an untitled buffer so the editor exists. The test workspace starts
 * empty with no open files, and both `setEditorValue` and `executePatch` are
 * silent no-ops without an active buffer.
 */
export async function openUntitledBuffer(window: Page): Promise<void> {
    await window.evaluate(() => window.__TEST_API__!.newUntitledFile());
    await window.waitForSelector('.monaco-editor', { timeout: 10_000 });
}
