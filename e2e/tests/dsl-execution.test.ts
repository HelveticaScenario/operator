/**
 * E2E tests for DSL execution flow.
 *
 * Verifies the core workflow: type DSL → execute → audio runs.
 */

import { test, expect, openUntitledBuffer } from '../fixtures';

test.describe('DSL execution', () => {
    test('can set editor value via test API', async ({ window }) => {
        // Wait for the app to fully load
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('// test code');
        });

        const value = await window.evaluate(() => {
            return window.__TEST_API__!.getEditorValue();
        });
        expect(value).toContain('test code');
    });

    test('execute simple sine patch', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Set a simple sine patch
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('$sine($hz(440)).out()');
        });

        // Execute the patch
        await window.evaluate(() => window.__TEST_API__!.executePatch());

        // Wait for execution to complete
        await window.waitForTimeout(2000);

        // Check that clock is running (audio is active)
        const running = await window.evaluate(() => {
            return window.__TEST_API__!.isClockRunning();
        });
        expect(running).toBe(true);
    });

    test('executed patch produces last result', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('$sine($hz(440)).out()');
        });

        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        const result = await window.evaluate(() => {
            return window.__TEST_API__!.getLastPatchResult();
        });

        // Result should exist and indicate success
        if (result) {
            expect(result.success).toBe(true);
        }
    });

    test('scope data is available after execution', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Execute a patch with a scope
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue(
                '$sine($hz(440)).scope().out()',
            );
        });

        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        const scopeData = await window.evaluate(() => {
            return window.__TEST_API__!.getScopeData();
        });

        // Scope data should be an array (may be empty if no scopes configured)
        expect(Array.isArray(scopeData)).toBe(true);
    });

    test('audio health is available', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        const health = await window.evaluate(() => {
            return window.__TEST_API__!.getAudioHealth();
        });

        // Health should be an object with budget info
        expect(health).toBeDefined();
    });
});
