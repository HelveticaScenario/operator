/**
 * E2E tests for caret stability while typing in the patch editor.
 *
 * Every keystroke round-trips through React state back into the editor's
 * `value`; a value that arrives after a newer keystroke must never be written
 * over the model, which would also move the caret to the end of the buffer.
 */

import { test, expect, openUntitledBuffer } from '../fixtures';

const SOURCE = [
    'const freq = 440',
    '$sine($hz(freq)).out()',
    '',
    '// trailing line',
    '// another trailing line',
].join('\n');

test.describe('editor caret', () => {
    test('stays put when Backspace and = are typed in quick succession', async ({
        window,
    }) => {
        await window.waitForTimeout(3000);

        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);
        await window.evaluate((code) => {
            window.__TEST_API__!.setEditorValue(code);
        }, SOURCE);
        // A running patch keeps the scope/meter render loop busy, which is
        // the load under which stale values reach the editor.
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(1000);

        // Caret just after `freq =` on line 1, i.e. before ` 440`.
        await window.evaluate(() => {
            const ed = (window as any).monaco.editor
                .getEditors()
                .find(
                    (e: any) => e.getModel()?.getLanguageId() === 'javascript',
                );
            ed.focus();
            ed.setPosition({ lineNumber: 1, column: 13 });
        });

        // Keystrokes go back to back: the race is between the `=` echo and
        // the following Backspace.
        for (let i = 0; i < 100; i++) {
            await window.keyboard.press('Backspace');
            await window.keyboard.press('=');
        }
        const state = await window.evaluate(() => {
            const ed = (window as any).monaco.editor
                .getEditors()
                .find(
                    (e: any) => e.getModel()?.getLanguageId() === 'javascript',
                );
            return {
                position: ed.getPosition(),
                value: ed.getModel().getValue(),
            };
        });
        expect(state.value).toBe(SOURCE);
        expect(state.position).toEqual({ lineNumber: 1, column: 13 });
    });
});
