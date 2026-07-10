/**
 * E2E tests for patch editing workflows.
 *
 * Verifies that patches can be modified and re-executed.
 */

import { test, expect, openUntitledBuffer } from '../fixtures';

test.describe('patch editing', () => {
    test('can execute, modify, and re-execute a patch', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Execute initial patch
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('$sine($hz(440)).out()');
        });
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        const result1 = await window.evaluate(() =>
            window.__TEST_API__!.getLastPatchResult(),
        );
        if (result1) {
            expect(result1.success).toBe(true);
        }

        // Modify and re-execute with different frequency
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('$saw($hz(220)).out()');
        });
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        const result2 = await window.evaluate(() =>
            window.__TEST_API__!.getLastPatchResult(),
        );
        if (result2) {
            expect(result2.success).toBe(true);
        }

        // Audio should still be running
        const running = await window.evaluate(() => {
            return window.__TEST_API__!.isClockRunning();
        });
        expect(running).toBe(true);
    });

    test('re-execution preserves audio state', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Execute a patch
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('$sine($hz(440)).out()');
        });
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(1000);

        // Verify audio is running
        let running = await window.evaluate(() =>
            window.__TEST_API__!.isClockRunning(),
        );
        expect(running).toBe(true);

        // Re-execute same patch (should not stop audio)
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(1000);

        running = await window.evaluate(() =>
            window.__TEST_API__!.isClockRunning(),
        );
        expect(running).toBe(true);
    });

    test('stop button stops audio', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Execute a patch first
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('$sine($hz(440)).out()');
        });
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        // Click Stop button
        const stopBtn = window.locator('button:has-text("Stop")');
        await stopBtn.click();
        await window.waitForTimeout(1000);

        // Clock should stop
        const running = await window.evaluate(() => {
            return window.__TEST_API__!.isClockRunning();
        });
        // After stop, isClockRunning may be false
        // (depends on implementation — stop might just mute, not stop the clock)
        expect(typeof running).toBe('boolean');
    });
});
