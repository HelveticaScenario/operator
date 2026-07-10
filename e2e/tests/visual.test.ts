/**
 * Visual regression E2E tests.
 *
 * Uses Playwright's screenshot comparison to detect unintended UI changes.
 * Update snapshots with: yarn test:e2e:update
 */

import { test, expect, openUntitledBuffer } from '../fixtures';

test.describe('visual regression', () => {
    test('main window appearance', async ({ window }) => {
        // Wait for full render
        await window.waitForTimeout(5000);

        // Take a full-window screenshot and compare against golden
        await expect(window).toHaveScreenshot('main-window.png', {
            maxDiffPixelRatio: 0.05,
            timeout: 10_000,
        });
    });

    test('editor area appearance', async ({ window }) => {
        await window.waitForTimeout(5000);

        const editorArea = window
            .locator('.editor-panel, .empty-state')
            .first();
        const isVisible = await editorArea.isVisible().catch(() => false);

        if (isVisible) {
            await expect(editorArea).toHaveScreenshot('editor-area.png', {
                maxDiffPixelRatio: 0.05,
                timeout: 10_000,
            });
        }
    });

    test('with active patch', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Set and execute a patch
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue(
                '$sine($hz(440)).scope().out()',
            );
        });
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(3000);

        // Screenshot with active patch and scope
        await expect(window).toHaveScreenshot('active-patch.png', {
            maxDiffPixelRatio: 0.1, // Higher tolerance for oscilloscope animation
            timeout: 10_000,
        });
    });
});
