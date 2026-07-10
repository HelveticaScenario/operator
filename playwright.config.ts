/**
 * Playwright E2E test configuration for the Operator Electron app.
 *
 * Usage:
 *   yarn test:e2e           — run all E2E tests
 *
 * Prerequisites:
 *   The Vite main/preload bundles must exist in .vite/build/ (run `yarn start`
 *   once to create them). The webServer config below serves the renderer on
 *   port 5173 (or reuses your existing `yarn start` dev server if it's
 *   already running).
 */

import { defineConfig } from '@playwright/test';

export default defineConfig({
    testDir: './e2e',
    timeout: 60_000,
    retries: 0,
    workers: 1, // Electron tests must run serially (one app instance at a time)
    reporter: [['list'], ['html', { open: 'never' }]],
    use: {
        trace: 'retain-on-failure',
        screenshot: 'only-on-failure',
    },

    /**
     * Serve the renderer with a Vite dev server on port 5173.
     *
     * The dev-built main process loads the renderer from
     * MAIN_WINDOW_VITE_DEV_SERVER_URL (http://localhost:5173, baked in by
     * electron-forge's VitePlugin). In dev mode (`yarn start`) forge runs this
     * server itself; `reuseExistingServer: true` picks it up when present.
     */
    webServer: {
        command:
            'yarn vite --config vite.renderer.config.ts --port 5173 --strictPort',
        port: 5173,
        reuseExistingServer: true,
        timeout: 30_000,
    },
});
