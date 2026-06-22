/**
 * Tests for formatPath, which turns a buffer identifier into the file:// model
 * URI Monaco uses. Extensionless/untitled buffers become .mjs so they are
 * treated as ES modules; real extensions (.js/.mjs/.json) are preserved.
 */
import { describe, expect, test } from 'vitest';
import { formatPath } from './monacoHelpers';

describe('formatPath', () => {
    test('preserves .json so JSON buffers keep a JSON model URI', () => {
        expect(formatPath('keybindings.json')).toBe('file:///keybindings.json');
        expect(formatPath('/Users/x/keybindings.json')).toBe(
            'file:///Users/x/keybindings.json',
        );
        expect(formatPath('config.json')).toBe('file:///config.json');
    });

    test('appends .mjs to extensionless / untitled buffers', () => {
        expect(formatPath('untitled-1')).toBe('file:///untitled-1.mjs');
    });

    test('preserves .js and .mjs extensions', () => {
        expect(formatPath('/abs/patch.js')).toBe('file:///abs/patch.js');
        expect(formatPath('/abs/patch.mjs')).toBe('file:///abs/patch.mjs');
    });
});
