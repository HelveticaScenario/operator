/**
 * E2E tests for editor buffer lifecycle.
 *
 * Buffer ids are reused (a new untitled buffer takes the lowest free number),
 * so a new buffer must never show a closed buffer's content. Saving never
 * silently overwrites a file another program changed on disk, and a buffer
 * without unsaved edits follows its file.
 */

import * as fs from 'fs';
import * as path from 'path';
import type { Page } from '@playwright/test';
import { test, expect, openUntitledBuffer } from '../fixtures';

async function setEditor(window: Page, code: string) {
    await window.evaluate(
        (value) => window.__TEST_API__!.setEditorValue(value),
        code,
    );
}

async function save(window: Page) {
    await window.locator('.monaco-editor').first().click();
    await window.keyboard.press('Meta+s');
}

test.describe('editor buffers', () => {
    test('a new untitled buffer does not show a closed buffer’s content', async ({
        electronApp,
        window,
    }) => {
        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        // Answer the unsaved-changes prompt with "Don't Save".
        await electronApp.evaluate(({ ipcMain }) => {
            ipcMain.removeHandler('ui:show-unsaved-changes-dialog');
            ipcMain.handle('ui:show-unsaved-changes-dialog', () => 1);
        });

        await openUntitledBuffer(window);
        const defaultContent = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );

        await window.evaluate(() =>
            window.__TEST_API__!.setEditorValue('// closed buffer content'),
        );
        // The close button only shows while its row is hovered.
        const row = window.locator('li', {
            has: window.locator('.close-button'),
        });
        await row.first().hover();
        await row.first().locator('.close-button').click();
        await expect(window.locator('.monaco-editor')).toHaveCount(0);

        await openUntitledBuffer(window);
        await expect
            .poll(() =>
                window.evaluate(() => window.__TEST_API__!.getEditorValue()),
            )
            .toBe(defaultContent);
    });

    test('saving over a file changed on disk asks to compare or overwrite', async ({
        electronApp,
        window,
    }) => {
        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await electronApp.evaluate(({ ipcMain }) => {
            ipcMain.removeHandler('modular:fs:show-save-dialog');
            ipcMain.handle('modular:fs:show-save-dialog', () => 'conflict.mjs');
        });
        const workspace = await window.evaluate(
            async () =>
                (await window.electronAPI.filesystem.getWorkspace())!.path,
        );
        const filePath = path.join(workspace, 'conflict.mjs');
        const notification = window.locator('.save-conflict-notification');

        await openUntitledBuffer(window);
        await setEditor(window, '// mine v1\n');
        await save(window);
        await expect.poll(() => fs.existsSync(filePath)).toBe(true);

        // Another program edits the file under unsaved edits, so a save from
        // the app is refused.
        await setEditor(window, '// mine v2\n');
        fs.writeFileSync(filePath, '// theirs v1\n');
        await save(window);
        await expect(notification).toContainText(
            "Failed to save 'conflict.mjs': The content of the file is newer.",
        );
        expect(fs.readFileSync(filePath, 'utf-8')).toBe('// theirs v1\n');

        // Compare, then discard local changes.
        await notification.getByRole('button', { name: 'Compare' }).click();
        await expect(window.locator('.save-conflict-panel h2')).toHaveText(
            'conflict.mjs (in file) ↔ conflict.mjs (in Operator) - Resolve save conflict',
        );
        await window.getByRole('button', { name: 'Use file contents' }).click();
        await expect(window.locator('.save-conflict-panel')).toHaveCount(0);
        await expect
            .poll(() =>
                window.evaluate(() => window.__TEST_API__!.getEditorValue()),
            )
            .toBe('// theirs v1\n');

        // A second external edit, resolved by overwriting.
        await setEditor(window, '// mine v3\n');
        fs.writeFileSync(filePath, '// theirs v2\n');
        await save(window);
        await notification.getByRole('button', { name: 'Overwrite' }).click();
        await expect(notification).toHaveCount(0);
        await expect
            .poll(() => fs.readFileSync(filePath, 'utf-8'))
            .toContain('// mine v3');
    });

    test('a buffer without unsaved edits reloads when its file changes on disk', async ({
        electronApp,
        window,
    }) => {
        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await electronApp.evaluate(({ ipcMain }) => {
            ipcMain.removeHandler('modular:fs:show-save-dialog');
            // An unnormalized spelling: change events must still reach the
            // buffer, which is keyed by the path exactly as the renderer built it.
            ipcMain.handle(
                'modular:fs:show-save-dialog',
                () => 'sub/../reload.mjs',
            );
        });
        const workspace = await window.evaluate(
            async () =>
                (await window.electronAPI.filesystem.getWorkspace())!.path,
        );
        const filePath = path.join(workspace, 'reload.mjs');
        const editorValue = () =>
            window.evaluate(() => window.__TEST_API__!.getEditorValue());

        await openUntitledBuffer(window);
        await setEditor(window, '// saved\n');
        await save(window);
        await expect.poll(() => fs.existsSync(filePath)).toBe(true);
        // The watch starts once the renderer has the saved file buffer.
        await expect(window.locator('.dirty-dot')).toHaveCount(0);
        await window.waitForTimeout(1000);

        // A plain write, then a replace-by-rename as atomic-saving editors do.
        fs.writeFileSync(filePath, '// external v1\n');
        await expect.poll(editorValue).toBe('// external v1\n');
        fs.writeFileSync(`${filePath}.tmp`, '// external v2\n');
        fs.renameSync(`${filePath}.tmp`, filePath);
        await expect.poll(editorValue).toBe('// external v2\n');
        await expect(window.locator('.dirty-dot')).toHaveCount(0);

        // Unsaved edits are kept.
        await setEditor(window, '// unsaved\n');
        fs.writeFileSync(filePath, '// external v3\n');
        await window.waitForTimeout(1000);
        expect(await editorValue()).toBe('// unsaved\n');
    });
});
