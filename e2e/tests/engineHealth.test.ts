/**
 * E2E tests for the Engine Health modal.
 */

import { test, expect } from '../fixtures';

test.describe('engine health modal', () => {
    test('engine health modal opens and displays content', async ({ window }) => {
        await window.waitForTimeout(2000);

        // Open via test API
        await window.evaluate(() => (window as any).__TEST_API__.openEngineHealth());
        await window.waitForTimeout(500);

        const panel = window.locator('.engine-health-panel');
        await expect(panel).toBeVisible();
        await expect(window.locator('text=Engine Health')).toBeVisible();
        await expect(window.locator('text=Audio CPU')).toBeVisible();
        await expect(window.locator('text=Average')).toBeVisible();
        await expect(window.locator('text=Peak')).toBeVisible();
    });

    test('engine health modal closes on Escape', async ({ window }) => {
        await window.waitForTimeout(2000);

        await window.evaluate(() => (window as any).__TEST_API__.openEngineHealth());
        await window.waitForTimeout(500);

        const panel = window.locator('.engine-health-panel');
        await expect(panel).toBeVisible();

        await window.keyboard.press('Escape');
        await window.waitForTimeout(500);
        await expect(panel).not.toBeVisible();
    });
});
