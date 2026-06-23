/**
 * Tests for the context-key service. The service is a global singleton,
 * so each test calls `reset()` in afterEach.
 */

import { afterEach, describe, expect, test } from 'vitest';

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

    test('setMany sets every entry at once', () => {
        contextKeys.setMany({ a: 1, b: 2, c: 3 });

        expect(contextKeys.get('a')).toBe(1);
        expect(contextKeys.get('b')).toBe(2);
        expect(contextKeys.get('c')).toBe(3);
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
