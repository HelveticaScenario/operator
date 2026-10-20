import { describe, expect, test } from 'vitest';
import { createModelValueSync } from './modelValueSync';

describe('createModelValueSync', () => {
    test('a value equal to the model is never written', () => {
        const sync = createModelValueSync();
        expect(sync.shouldWrite('a', 'a')).toBe(false);
    });

    test('an echo older than the model is never written', () => {
        const sync = createModelValueSync();
        // `=` then Backspace: the `=` echo arrives after the Backspace.
        sync.noteEmitted('x = 1');
        sync.noteEmitted('x  1');
        expect(sync.shouldWrite('x = 1', 'x  1')).toBe(false);
        expect(sync.shouldWrite('x  1', 'x  1')).toBe(false);
    });

    test('echoes skipped by a batched render are dropped', () => {
        const sync = createModelValueSync();
        sync.noteEmitted('a');
        sync.noteEmitted('ab');
        sync.noteEmitted('abc');
        expect(sync.shouldWrite('ab', 'abc')).toBe(false);
        // `a` was skipped; it is no longer an echo.
        expect(sync.shouldWrite('a', 'abc')).toBe(true);
    });

    test('an echo of text the model returns to through undo is not written', () => {
        const sync = createModelValueSync();
        sync.noteEmitted('a');
        sync.noteEmitted('ab');
        sync.noteEmitted('a');
        expect(sync.shouldWrite('ab', 'a')).toBe(false);
        expect(sync.shouldWrite('a', 'a')).toBe(false);
    });

    test('a value the editor did not emit is written', () => {
        const sync = createModelValueSync();
        sync.noteEmitted('typed');
        expect(sync.shouldWrite('reloaded from disk', 'typed')).toBe(true);
    });

    test('a value matching the model clears pending echoes', () => {
        const sync = createModelValueSync();
        sync.noteEmitted('old');
        sync.noteEmitted('new');
        expect(sync.shouldWrite('new', 'new')).toBe(false);
        expect(sync.shouldWrite('old', 'new')).toBe(true);
    });

    test('reset forgets emitted text', () => {
        const sync = createModelValueSync();
        sync.noteEmitted('other buffer');
        sync.reset();
        expect(sync.shouldWrite('other buffer', 'this buffer')).toBe(true);
    });
});
