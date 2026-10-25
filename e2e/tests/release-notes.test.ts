/**
 * E2E tests for the release notes viewer: notes offered with an update, the
 * "What's new" panel after updating, and Help → Release Notes.
 *
 * The update check runs for real against a stubbed `fetch` in the main
 * process. The bundled changelog has no notes yet, so the What's new and Help
 * tests replace those IPC handlers with fixture notes.
 */

import * as fs from 'fs';
import * as path from 'path';
import type { ElectronApplication } from 'playwright';
import { test, expect } from '../fixtures';

const INSTALLED_VERSION = '0.0.130';

const RELEASES = [
    {
        body: 'Unstable build.\n\n### New\n\n- Not shown.',
        draft: false,
        html_url:
            'https://github.com/HelveticaScenario/operator/releases/tag/v0.3.0-beta.1',
        prerelease: true,
        published_at: '2026-11-20T12:00:00Z',
        tag_name: 'v0.3.0-beta.1',
    },
    {
        body: '### New\n\n- Not shown either.',
        draft: true,
        html_url:
            'https://github.com/HelveticaScenario/operator/releases/tag/v0.4.0',
        prerelease: false,
        published_at: null,
        tag_name: 'v0.4.0',
    },
    {
        body: [
            '### Breaking',
            '',
            '- `$foo` is now called `$bar`. Update your patches:',
            '  ',
            '  ```js',
            "  $bar('c4').out()",
            '  ```',
            '',
            '### New',
            '',
            '- A plate reverb, `$plate`. See the [guide](https://example.com/plate).',
            '- <img src="https://example.com/x.png" onerror="alert(1)"> Raw HTML is dropped.',
            '',
            '### Fixed',
            '',
            '- Scopes no longer flicker when you resize the window.',
            '',
        ].join('\n'),
        draft: false,
        html_url:
            'https://github.com/HelveticaScenario/operator/releases/tag/v0.2.0',
        prerelease: false,
        published_at: '2026-11-02T12:00:00Z',
        tag_name: 'v0.2.0',
    },
    {
        body: '### Improved\r\n\r\n- Patches load faster.\r\n',
        draft: false,
        html_url:
            'https://github.com/HelveticaScenario/operator/releases/tag/v0.1.1',
        prerelease: false,
        published_at: '2026-10-30T12:00:00Z',
        tag_name: 'v0.1.1',
    },
    {
        body: '### Breaking\n\n- Envelopes take times in seconds.\n',
        draft: false,
        html_url:
            'https://github.com/HelveticaScenario/operator/releases/tag/v0.1.0',
        prerelease: false,
        published_at: '2026-10-24T12:00:00Z',
        tag_name: 'v0.1.0',
    },
    {
        body: 'Some unrelated text from an old release.',
        draft: false,
        html_url:
            'https://github.com/HelveticaScenario/operator/releases/tag/v0.0.130',
        prerelease: false,
        published_at: '2026-10-01T12:00:00Z',
        tag_name: 'v0.0.130',
    },
];

const FIXTURE_NOTES = [
    {
        date: 'November 2, 2026',
        sections: {
            Breaking: ['`$foo` is now called `$bar`.'],
            New: ['A plate reverb, `$plate`.'],
        },
        version: '0.2.0',
    },
    {
        date: 'October 30, 2026',
        sections: { Fixed: ['Scopes no longer flicker.'] },
        version: '0.1.1',
    },
];

/** Pins the app version and records external opens instead of launching a browser. */
async function stubMainProcess(electronApp: ElectronApplication) {
    await electronApp.evaluate(
        ({ app, shell }, { version, releases }) => {
            app.getVersion = () => version;
            const opened: string[] = [];
            (globalThis as Record<string, unknown>).__openedUrls = opened;
            shell.openExternal = async (url: string) => {
                opened.push(url);
            };
            globalThis.fetch = async () =>
                new Response(JSON.stringify(releases), {
                    headers: { 'Content-Type': 'application/json' },
                    status: 200,
                });
        },
        { releases: RELEASES, version: INSTALLED_VERSION },
    );
}

async function replaceHandler(
    electronApp: ElectronApplication,
    channel: string,
    result: unknown,
) {
    await electronApp.evaluate(
        ({ ipcMain }, { channel, result }) => {
            ipcMain.removeHandler(channel);
            ipcMain.handle(channel, () => result);
        },
        { channel, result },
    );
}

test.describe('release notes', () => {
    test('a fresh install records its version and shows nothing', async ({
        electronApp,
        window,
    }) => {
        await window.waitForTimeout(1500);
        await expect(window.locator('.release-notes-panel')).toHaveCount(0);

        const userData = await electronApp.evaluate(({ app }) =>
            app.getPath('userData'),
        );
        const version = await electronApp.evaluate(({ app }) =>
            app.getVersion(),
        );
        const config = JSON.parse(
            fs.readFileSync(path.join(userData, 'config.json'), 'utf8'),
        );
        expect(config.lastSeenVersion).toBe(version);
    });

    test('an update shows the notes of every newer release', async ({
        electronApp,
        window,
    }, testInfo) => {
        await stubMainProcess(electronApp);
        await window.evaluate(() => window.electronAPI.update.check());

        const bar = window.locator('.update-notification');
        await expect(bar).toContainText('Version 0.2.0 is available.');
        await expect(bar).toContainText('Includes breaking changes');
        await window.screenshot({
            path: testInfo.outputPath('update-bar.png'),
        });

        await bar.getByRole('button', { name: "What's New" }).click();
        const panel = window.locator('.release-notes-panel');
        await expect(panel).toBeVisible();
        await expect(panel).toContainText('Version 0.2.0 is available');
        await expect(panel).toContainText('You have version 0.0.130.');

        const breaking = panel.locator('.release-notes-breaking li');
        await expect(breaking).toHaveCount(2);
        await expect(breaking.nth(0)).toContainText('0.2.0');
        await expect(breaking.nth(0)).toContainText("$bar('c4').out()");
        await expect(breaking.nth(1)).toContainText('0.1.0');

        const releases = panel.locator('.release-notes-release h3');
        await expect(releases).toHaveText([
            /Version 0\.2\.0/,
            /Version 0\.1\.1/,
        ]);
        await expect(panel).not.toContainText('Not shown');
        await expect(panel.locator('img')).toHaveCount(0);
        await expect(panel).toContainText('Raw HTML is dropped.');

        await panel.screenshot({
            path: testInfo.outputPath('update-notes.png'),
        });

        const pageUrl = window.url();
        await panel.getByRole('link', { name: 'guide' }).click();
        await expect
            .poll(() =>
                electronApp.evaluate(
                    () =>
                        (globalThis as Record<string, unknown>)
                            .__openedUrls as string[],
                ),
            )
            .toEqual(['https://example.com/plate']);
        expect(window.url()).toBe(pageUrl);

        await panel.getByRole('button', { name: 'Not Now' }).click();
        await expect(panel).toHaveCount(0);
        await expect(bar).toBeVisible();
    });

    test("What's new appears after updating", async ({
        electronApp,
        window,
    }, testInfo) => {
        await replaceHandler(electronApp, 'modular:release-notes:whats-new', {
            currentVersion: '0.2.0',
            releaseNotes: FIXTURE_NOTES,
        });
        await window.reload();

        const panel = window.locator('.release-notes-panel');
        await expect(panel).toContainText("What's new in version 0.2.0");
        await expect(panel.locator('.release-notes-breaking li')).toHaveCount(
            1,
        );
        await expect(panel.locator('.release-notes-release h3')).toHaveText([
            /Version 0\.2\.0/,
            /Version 0\.1\.1/,
        ]);
        await panel.screenshot({
            path: testInfo.outputPath('whats-new.png'),
        });

        await window.keyboard.press('Escape');
        await expect(panel).toHaveCount(0);
    });

    test('Help → Release Notes opens the changelog', async ({
        electronApp,
        window,
    }, testInfo) => {
        await replaceHandler(
            electronApp,
            'modular:release-notes:get-all',
            FIXTURE_NOTES,
        );
        await electronApp.evaluate(({ Menu }) => {
            const help = Menu.getApplicationMenu()?.items.find(
                (item) => item.role === 'help',
            );
            help?.submenu?.items
                .find((item) => item.label === 'Release Notes')
                ?.click();
        });

        const panel = window.locator('.release-notes-panel');
        await expect(panel).toContainText('Release Notes');
        await expect(panel.locator('.release-notes-release h3')).toHaveText([
            /Version 0\.2\.0/,
            /Version 0\.1\.1/,
        ]);
        await panel.screenshot({
            path: testInfo.outputPath('changelog.png'),
        });

        await panel.getByRole('button', { name: 'Close' }).first().click();
        await expect(panel).toHaveCount(0);
    });
});
