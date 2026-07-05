import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { createConfigStore, parseConfig } from '../appConfig';

describe('parseConfig', () => {
    test('keeps valid fields when one field is invalid', () => {
        const config = parseConfig({
            fontSize: 7, // below the schema minimum of 8
            lastOpenedFolder: '/Users/me/patches',
            theme: 'modular-dark',
        });
        expect(config).toEqual({
            lastOpenedFolder: '/Users/me/patches',
            theme: 'modular-dark',
        });
    });

    test('passes unknown fields through untouched', () => {
        const config = parseConfig({
            futureSetting: { nested: true },
            theme: 'modular-dark',
        });
        expect(config).toEqual({
            futureSetting: { nested: true },
            theme: 'modular-dark',
        });
    });

    test('an invalid nested field drops only that field, keeping siblings', () => {
        const config = parseConfig({
            audioConfig: {
                hostId: 'coreaudio',
                outputDeviceId: 'X',
                sampleRate: '48000', // string, schema wants a number
            },
        });
        expect(config).toEqual({
            audioConfig: { hostId: 'coreaudio', outputDeviceId: 'X' },
        });
    });

    test('unknown nested keys inside known fields pass through untouched', () => {
        const config = parseConfig({
            audioConfig: { exclusiveMode: true, hostId: 'coreaudio' },
        });
        expect(config).toEqual({
            audioConfig: { exclusiveMode: true, hostId: 'coreaudio' },
        });
    });

    test('returns null for a non-object root', () => {
        expect(parseConfig(null)).toBeNull();
        expect(parseConfig([1, 2])).toBeNull();
        expect(parseConfig('theme')).toBeNull();
    });
});

describe('createConfigStore', () => {
    let dir: string;
    let configFile: string;

    beforeEach(() => {
        dir = fs.mkdtempSync(path.join(os.tmpdir(), 'appconfig-test-'));
        configFile = path.join(dir, 'config.json');
        vi.spyOn(console, 'error').mockImplementation(() => {});
    });

    afterEach(() => {
        vi.restoreAllMocks();
        fs.rmSync(dir, { force: true, recursive: true });
    });

    test('one invalid field survives a load-merge-save cycle without losing the rest', () => {
        fs.writeFileSync(
            configFile,
            JSON.stringify({
                fontSize: 7,
                lastOpenedFolder: '/Users/me/patches',
                skippedUpdateVersion: '1.2.3',
                theme: 'modular-dark',
            }),
        );
        const store = createConfigStore(configFile);

        store.update((config) => {
            config.audioConfig = { hostId: 'coreaudio', sampleRate: 48000 };
        });

        const onDisk = JSON.parse(fs.readFileSync(configFile, 'utf-8'));
        expect(onDisk).toEqual({
            audioConfig: { hostId: 'coreaudio', sampleRate: 48000 },
            lastOpenedFolder: '/Users/me/patches',
            skippedUpdateVersion: '1.2.3',
            theme: 'modular-dark',
        });
    });

    test('update never rewrites an unparseable config file', () => {
        fs.writeFileSync(configFile, '{ not json');
        const store = createConfigStore(configFile);

        expect(store.load()).toEqual({});
        store.update((config) => {
            config.theme = 'modular-dark';
        });

        expect(fs.readFileSync(configFile, 'utf-8')).toBe('{ not json');
    });

    test('update on a missing file creates it', () => {
        const store = createConfigStore(configFile);
        store.update((config) => {
            config.theme = 'modular-dark';
        });
        expect(JSON.parse(fs.readFileSync(configFile, 'utf-8'))).toEqual({
            theme: 'modular-dark',
        });
    });

    test('watcher keeps reporting changes across atomic saves', async () => {
        const store = createConfigStore(configFile);
        store.save({ fontSize: 12 });

        const seen: unknown[] = [];
        const watcher = store.watch((config) => {
            seen.push(config);
        });
        try {
            const atomicSave = (contents: string) => {
                const tmp = path.join(dir, 'config.json.tmp');
                fs.writeFileSync(tmp, contents);
                fs.renameSync(tmp, configFile);
            };
            const waitFor = async (predicate: () => boolean) => {
                const deadline = Date.now() + 5000;
                while (!predicate() && Date.now() < deadline) {
                    await new Promise((resolve) => setTimeout(resolve, 25));
                }
                expect(predicate()).toBe(true);
            };

            // fs.watch's underlying OS stream starts asynchronously, so a
            // change landing before it is live is never reported. Prime the
            // watcher by rewriting until an event arrives, then assert on
            // the saves that matter.
            const primeDeadline = Date.now() + 5000;
            while (seen.length === 0 && Date.now() < primeDeadline) {
                fs.writeFileSync(configFile, JSON.stringify({ fontSize: 12 }));
                await new Promise((resolve) => setTimeout(resolve, 50));
            }
            expect(seen.length).toBeGreaterThan(0);

            atomicSave(JSON.stringify({ fontSize: 14 }));
            await waitFor(() =>
                seen.some((c) => (c as { fontSize?: number }).fontSize === 14),
            );

            // A second atomic save must still be observed: the rename swapped
            // the file's inode, which detaches an inode-based watcher.
            atomicSave(JSON.stringify({ fontSize: 16 }));
            await waitFor(() =>
                seen.some((c) => (c as { fontSize?: number }).fontSize === 16),
            );
        } finally {
            watcher.close();
        }
    });
});
