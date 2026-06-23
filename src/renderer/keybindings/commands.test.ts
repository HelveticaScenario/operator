/**
 * Tests for the Operator command registry. The registry is a global
 * singleton, so each test cleans up after itself via `unregisterCommand`.
 */

import { afterEach, describe, expect, test, vi } from 'vitest';
import {
    executeCommand,
    getCommand,
    listCommands,
    registerCommand,
    unregisterCommand,
} from './commands';

const TEST_IDS = [
    'test.cmd.a',
    'test.cmd.b',
    'test.cmd.c',
    'test.cmd.async',
];

afterEach(() => {
    for (const id of TEST_IDS) {
        unregisterCommand(id);
    }
    vi.restoreAllMocks();
});

describe('command registry', () => {
    test('register + execute round-trip invokes the handler with args', () => {
        const handler = vi.fn();
        registerCommand('test.cmd.a', handler, {
            label: 'Test A',
            category: 'Tests',
        });

        executeCommand('test.cmd.a', 1, 'two', { three: true });

        expect(handler).toHaveBeenCalledTimes(1);
        expect(handler).toHaveBeenCalledWith(1, 'two', { three: true });
    });

    test('execute awaits an async handler via the returned promise', async () => {
        let resolved = false;
        registerCommand('test.cmd.async', async () => {
            await Promise.resolve();
            resolved = true;
        });
        await (executeCommand('test.cmd.async') as Promise<void>);
        expect(resolved).toBe(true);
    });

    test('executeCommand on unknown id throws with id in the message', () => {
        expect(() => executeCommand('test.cmd.missing')).toThrowError(
            /test\.cmd\.missing/,
        );
    });

    test('unregisterCommand removes the entry', () => {
        registerCommand('test.cmd.b', () => {});
        expect(getCommand('test.cmd.b')).toBeDefined();

        const removed = unregisterCommand('test.cmd.b');
        expect(removed).toBe(true);
        expect(getCommand('test.cmd.b')).toBeUndefined();
        expect(() => executeCommand('test.cmd.b')).toThrowError(/test\.cmd\.b/);
    });

    test('unregisterCommand on unknown id returns false', () => {
        expect(unregisterCommand('test.cmd.never-registered')).toBe(false);
    });

    test('re-registering an id warns and overwrites with the new handler', () => {
        const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
        const first = vi.fn();
        const second = vi.fn();

        registerCommand('test.cmd.a', first);
        registerCommand('test.cmd.a', second);

        expect(warn).toHaveBeenCalledTimes(1);
        expect(warn.mock.calls[0]?.[0]).toMatch(/test\.cmd\.a/);

        executeCommand('test.cmd.a');
        expect(first).not.toHaveBeenCalled();
        expect(second).toHaveBeenCalledTimes(1);
    });

    test('getCommand returns handler and metadata for registered ids', () => {
        const handler = () => {};
        const metadata = {
            label: 'Test C',
            category: 'Tests',
            when: 'editorFocused',
            contextMenu: { group: 'navigation', order: 1 },
        };
        registerCommand('test.cmd.c', handler, metadata);

        const entry = getCommand('test.cmd.c');
        expect(entry).toBeDefined();
        expect(entry?.handler).toBe(handler);
        expect(entry?.metadata).toEqual(metadata);
    });

    test('listCommands returns every registered id and metadata', () => {
        registerCommand('test.cmd.a', () => {}, { label: 'Test A' });
        registerCommand('test.cmd.b', () => {}, {
            label: 'Test B',
            category: 'Tests',
        });

        const ids = new Map(
            listCommands().map((c) => [c.id, c.metadata] as const),
        );
        expect(ids.get('test.cmd.a')).toEqual({ label: 'Test A' });
        expect(ids.get('test.cmd.b')).toEqual({
            label: 'Test B',
            category: 'Tests',
        });
    });
});
