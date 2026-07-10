/**
 * E2E tests for the VU meter panel: per-output meters, mute/solo buttons
 * (source edit + live gate update), code-only Ctrl/Cmd edits with ghost
 * state, and panel toggling.
 */

import { test, expect, openUntitledBuffer } from '../fixtures';

const TWO_OUT_PATCH = [
    `$sine($hz(440)).out({ label: 'lead' })`,
    `$saw($hz(110)).outMono(0, { label: 'bass' })`,
].join('\n');

/** Sum of a frame's per-channel rms for the given tap module id. */
async function rmsFor(
    window: import('playwright').Page,
    tapModuleId: string,
): Promise<number> {
    return window.evaluate(async (id) => {
        const frames = await window.__TEST_API__!.getVuMeterData();
        const frame = frames.find((f: any) => f.moduleId === id);
        if (!frame) {
            return 0;
        }
        return frame.rms.reduce((a: number, b: number) => a + b, 0);
    }, tapModuleId);
}

async function setupTwoOutPatch(window: import('playwright').Page) {
    await window.waitForTimeout(3000);
    const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
    test.skip(!hasTestAPI, '__TEST_API__ not available');

    await openUntitledBuffer(window);
    await window.evaluate((patch) => {
        window.__TEST_API__!.setEditorValue(patch);
    }, TWO_OUT_PATCH);
    await window.evaluate(() => window.__TEST_API__!.executePatch());
    await window.waitForTimeout(2000);

    await window.evaluate(() => {
        window.__TEST_API__!.setVuPanelVisible(true);
    });
    await window.waitForTimeout(500);
}

test.describe('vu meter panel', () => {
    test('shows one labeled meter per out', async ({ window }) => {
        await setupTwoOutPatch(window);

        await expect(window.locator('.vu-meter-panel')).toBeVisible();
        // Two channel strips plus the pinned end-of-chain master strip.
        await expect(window.locator('.vu-meter')).toHaveCount(3);
        await expect(window.locator('.vu-meter--main')).toBeVisible();
        await expect(
            window.locator('.vu-meter[data-vu-key="lead"] .vu-meter-label'),
        ).toHaveText('lead');
        await expect(
            window.locator('.vu-meter[data-vu-key="bass"] .vu-meter-label'),
        ).toHaveText('bass');

        // Both outputs report live loudness.
        await expect
            .poll(() => rmsFor(window, '__vuTap_lead'), { timeout: 10_000 })
            .toBeGreaterThan(0.01);
        await expect
            .poll(() => rmsFor(window, '__vuTap_bass'), { timeout: 10_000 })
            .toBeGreaterThan(0.01);
    });

    test('mute edits the source, greys the meter, keeps it metering', async ({
        window,
    }) => {
        await setupTwoOutPatch(window);

        await window
            .locator('.vu-meter[data-vu-key="lead"] .vu-btn-mute')
            .click();
        await window.waitForTimeout(300);

        const source = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(source).toContain(`out({ label: 'lead', mute: true })`);

        await expect(
            window.locator('.vu-meter[data-vu-key="lead"]'),
        ).toHaveClass(/vu-meter--suppressed/);

        // The tap sits before the mute gate, so a muted output keeps
        // metering — that is the feature.
        await window.waitForTimeout(1000);
        expect(await rmsFor(window, '__vuTap_lead')).toBeGreaterThan(0.01);

        // Un-mute removes the property again.
        await window
            .locator('.vu-meter[data-vu-key="lead"] .vu-btn-mute')
            .click();
        await window.waitForTimeout(300);
        const restored = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(restored).not.toContain('mute');
        await expect(
            window.locator('.vu-meter[data-vu-key="lead"]'),
        ).not.toHaveClass(/vu-meter--suppressed/);
    });

    test('solo suppresses the other output and edits the source', async ({
        window,
    }) => {
        await setupTwoOutPatch(window);

        await window
            .locator('.vu-meter[data-vu-key="bass"] .vu-btn-solo')
            .click();
        await window.waitForTimeout(300);

        const source = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(source).toContain(
            `outMono(0, { label: 'bass', solo: true })`,
        );

        await expect(
            window.locator('.vu-meter[data-vu-key="lead"]'),
        ).toHaveClass(/vu-meter--suppressed/);
        await expect(
            window.locator('.vu-meter[data-vu-key="bass"]'),
        ).not.toHaveClass(/vu-meter--suppressed/);
    });

    test('mute state derives from the source across re-evals', async ({
        window,
    }) => {
        await setupTwoOutPatch(window);

        await window
            .locator('.vu-meter[data-vu-key="lead"] .vu-btn-mute')
            .click();
        await window.waitForTimeout(300);

        // Re-run the patch: the edited source recompiles to the same state.
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);

        const outputs = await window.evaluate(() =>
            window.__TEST_API__!.getVuOutputs(),
        );
        const lead = outputs.find((o: any) => o.key === 'lead');
        expect(lead.mute).toBe(true);
        await expect(
            window.locator('.vu-meter[data-vu-key="lead"]'),
        ).toHaveClass(/vu-meter--suppressed/);
    });

    test('cmd-click mute edits the source only until a patch update applies it', async ({
        window,
    }) => {
        await setupTwoOutPatch(window);

        const muteButton = window.locator(
            '.vu-meter[data-vu-key="lead"] .vu-btn-mute',
        );

        // Code-only gesture: the source gains mute: true...
        await muteButton.click({ modifiers: ['ControlOrMeta'] });
        await window.waitForTimeout(300);
        const source = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(source).toContain(`out({ label: 'lead', mute: true })`);

        // ...but the audio is untouched: the meter stays live and the
        // button's inner (audio) section stays unmuted while the outer
        // (code) section shows the pending mute.
        await expect(
            window.locator('.vu-meter[data-vu-key="lead"]'),
        ).not.toHaveClass(/vu-meter--suppressed/);
        await expect(muteButton).toHaveAttribute('aria-pressed', 'false');
        await expect(muteButton).toHaveAttribute(
            'data-code-pressed',
            'true',
        );

        // A patch update compiles the edited source and re-syncs audio to
        // code, clearing the ghost.
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(2000);
        await expect(
            window.locator('.vu-meter[data-vu-key="lead"]'),
        ).toHaveClass(/vu-meter--suppressed/);
        await expect(muteButton).toHaveAttribute('aria-pressed', 'true');
        await expect(muteButton).toHaveAttribute(
            'data-code-pressed',
            'true',
        );
    });

    test('cmd-click toggles the code state from the ghost, not the audio', async ({
        window,
    }) => {
        await setupTwoOutPatch(window);

        const muteButton = window.locator(
            '.vu-meter[data-vu-key="lead"] .vu-btn-mute',
        );

        // Two code-only clicks cancel out: the second toggles the ghost
        // back, restoring the source and re-syncing the outer section.
        await muteButton.click({ modifiers: ['ControlOrMeta'] });
        await window.waitForTimeout(300);
        await muteButton.click({ modifiers: ['ControlOrMeta'] });
        await window.waitForTimeout(300);

        const source = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(source).not.toContain('mute');
        await expect(muteButton).toHaveAttribute('aria-pressed', 'false');
        await expect(muteButton).toHaveAttribute(
            'data-code-pressed',
            'false',
        );
    });

    test('cmd-right-click reverts the code gain to the audio value', async ({
        window,
    }) => {
        await setupTwoOutPatch(window);

        const canvas = window.locator(
            '.vu-meter[data-vu-key="lead"] .vu-meter-canvas',
        );

        // Code-only gain jump: the source gains an explicit gain option
        // while the audio keeps running at unity.
        await canvas.click({
            modifiers: ['ControlOrMeta'],
            position: { x: 20, y: 15 },
        });
        await window.waitForTimeout(300);
        const edited = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(edited).toContain('gain:');
        const outputs = await window.evaluate(() =>
            window.__TEST_API__!.getVuOutputs(),
        );
        expect(outputs.find((o: any) => o.key === 'lead').gain).toBe(5);

        // Cmd-right-click reverts the source to the audio's unity gain
        // (the property is removed) and drops the ghost.
        await canvas.click({
            button: 'right',
            modifiers: ['ControlOrMeta'],
            position: { x: 20, y: 15 },
        });
        await window.waitForTimeout(300);
        const reverted = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(reverted).not.toContain('gain:');
        expect(reverted).toContain(`out({ label: 'lead' })`);
    });

    test('right-click resets the gain to unity in audio and code', async ({
        window,
    }) => {
        await setupTwoOutPatch(window);

        const canvas = window.locator(
            '.vu-meter[data-vu-key="lead"] .vu-meter-canvas',
        );

        // Plain click sets both the audio and the source to the clicked
        // position.
        await canvas.click({ position: { x: 20, y: 15 } });
        await window.waitForTimeout(300);
        const edited = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(edited).toContain('gain:');
        let outputs = await window.evaluate(() =>
            window.__TEST_API__!.getVuOutputs(),
        );
        expect(outputs.find((o: any) => o.key === 'lead').gain).not.toBe(5);

        // Plain right-click resets both back to unity; the source drops
        // the property.
        await canvas.click({
            button: 'right',
            position: { x: 20, y: 15 },
        });
        await window.waitForTimeout(300);
        const reset = await window.evaluate(() =>
            window.__TEST_API__!.getEditorValue(),
        );
        expect(reset).not.toContain('gain:');
        outputs = await window.evaluate(() =>
            window.__TEST_API__!.getVuOutputs(),
        );
        expect(outputs.find((o: any) => o.key === 'lead').gain).toBe(5);
    });

    test('panel visibility toggles', async ({ window }) => {
        await setupTwoOutPatch(window);
        await expect(window.locator('.vu-meter-panel')).toBeVisible();

        await window.evaluate(() => {
            window.__TEST_API__!.setVuPanelVisible(false);
        });
        await expect(window.locator('.vu-meter-panel')).not.toBeVisible();
    });

    test('placeholder shows when the patch has no outs', async ({
        window,
    }) => {
        await window.waitForTimeout(3000);
        const hasTestAPI = await window.evaluate(() => !!window.__TEST_API__);
        test.skip(!hasTestAPI, '__TEST_API__ not available');

        await openUntitledBuffer(window);
        await window.evaluate(() => {
            window.__TEST_API__!.setEditorValue('$sine($hz(440))');
        });
        await window.evaluate(() => window.__TEST_API__!.executePatch());
        await window.waitForTimeout(1500);
        await window.evaluate(() => {
            window.__TEST_API__!.setVuPanelVisible(true);
        });

        await expect(window.locator('.vu-meter-empty')).toBeVisible();
    });
});
