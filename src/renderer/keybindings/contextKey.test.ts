/**
 * Tests for the context-key service. The service is a global singleton,
 * so each test calls `reset()` in afterEach.
 */

import { afterEach, describe, expect, test, vi } from 'vitest';

import { contextKeys, evaluateWhen } from './contextKey';

afterEach(() => {
    contextKeys.reset();
});

describe('ContextKeyService', () => {
    test('set / get round-trip', () => {
        contextKeys.set('editorFocused', true);
        expect(contextKeys.get('editorFocused')).toBe(true);

        contextKeys.set('editorFocused', false);
        expect(contextKeys.get('editorFocused')).toBe(false);
    });

    test('unset removes the key', () => {
        contextKeys.set('inSettingsModal', true);
        contextKeys.unset('inSettingsModal');
        expect(contextKeys.get('inSettingsModal')).toBeUndefined();
    });

    test('setting the same value does not fire a change event', () => {
        const listener = vi.fn();
        contextKeys.onDidChange(listener);

        contextKeys.set('a', 1);
        contextKeys.set('a', 1);

        expect(listener).toHaveBeenCalledTimes(1);
        expect(listener).toHaveBeenCalledWith(new Set(['a']));
    });

    test('setMany emits a single change event listing changed keys only', () => {
        contextKeys.set('a', 1);
        const listener = vi.fn();
        contextKeys.onDidChange(listener);

        contextKeys.setMany({ a: 1, b: 2, c: 3 });

        expect(listener).toHaveBeenCalledTimes(1);
        expect(listener).toHaveBeenCalledWith(new Set(['b', 'c']));
    });

    test('setMany skips emit when nothing changed', () => {
        contextKeys.setMany({ a: 1, b: 2 });
        const listener = vi.fn();
        contextKeys.onDidChange(listener);

        contextKeys.setMany({ a: 1, b: 2 });

        expect(listener).not.toHaveBeenCalled();
    });

    test('onDidChange returns a Disposable that detaches the listener', () => {
        const listener = vi.fn();
        const handle = contextKeys.onDidChange(listener);

        contextKeys.set('a', 1);
        handle.dispose();
        contextKeys.set('a', 2);

        expect(listener).toHaveBeenCalledTimes(1);
    });

    test('listener exceptions do not break notification of other listeners', () => {
        const consoleErr = vi
            .spyOn(console, 'error')
            .mockImplementation(() => {});
        const a = vi.fn(() => {
            throw new Error('boom');
        });
        const b = vi.fn();
        contextKeys.onDidChange(a);
        contextKeys.onDidChange(b);

        contextKeys.set('a', 1);

        expect(a).toHaveBeenCalled();
        expect(b).toHaveBeenCalled();
        consoleErr.mockRestore();
    });

    test('snapshot returns a shallow copy of current values', () => {
        contextKeys.setMany({ a: 1, b: 'two' });
        const snap = contextKeys.snapshot();
        expect(snap).toEqual({ a: 1, b: 'two' });

        snap.a = 999;
        expect(contextKeys.get('a')).toBe(1);
    });

    test('evaluateWhen reads through the global service', () => {
        contextKeys.set('editorFocused', true);
        expect(evaluateWhen('editorFocused')).toBe(true);

        contextKeys.set('editorFocused', false);
        expect(evaluateWhen('editorFocused')).toBe(false);

        expect(evaluateWhen('')).toBe(true);
        expect(evaluateWhen(undefined)).toBe(true);
    });
});
