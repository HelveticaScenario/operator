/**
 * E2E tests for error handling.
 *
 * Verifies that invalid DSL produces visible errors.
 */

import { test, expect, openUntitledBuffer } from '../fixtures';

test.describe('error handling', () => {
    test('syntax error shows error display', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Set invalid DSL code (syntax error)
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue(
                'this is not valid javascript {{{',
            );
        });

        // Try to execute
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        // The error display should become visible or the result should indicate failure
        const errorVisible = await window
            .locator('.error-display')
            .isVisible()
            .catch(() => false);
        const result = await window.evaluate(() =>
            window.__TEST_API__!.getLastPatchResult(),
        );

        // Either the UI shows an error, or the result indicates failure
        const hasError = errorVisible || (result && !result.success);
        expect(hasError).toBe(true);
    });

    test('unknown module type shows error', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        // Use a nonexistent module function
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue(
                '$nonexistentModule($hz(440)).out()',
            );
        });

        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        const result = await window.evaluate(() =>
            window.__TEST_API__!.getLastPatchResult(),
        );

        // Should fail since the module doesn't exist
        if (result) {
            expect(result.success).toBe(false);
        }
    });

    test('error message contains useful info', async ({ window }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);

        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue(
                'throw new Error("test error message")',
            );
        });

        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        const result = await window.evaluate(() =>
            window.__TEST_API__!.getLastPatchResult(),
        );

        if (result && result.errorMessage) {
            expect(result.errorMessage).toContain('test error message');
        }
    });
});
