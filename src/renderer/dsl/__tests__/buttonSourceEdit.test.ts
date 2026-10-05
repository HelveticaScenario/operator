import { describe, expect, test } from 'vitest';

import { findToggleBtnStateSpan } from '../buttonSourceEdit';

describe('findToggleBtnStateSpan', () => {
    test('locates the state literal of a single toggle button', () => {
        const source = `$saw('c2').amplitude($toggleBtn('drone', false)).out();`;
        const span = findToggleBtnStateSpan(source, 'drone');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('false');
    });

    test('supports double-quoted labels and true literals', () => {
        const source = `$saw('c2').amplitude($toggleBtn("drone", true)).out();`;
        const span = findToggleBtnStateSpan(source, 'drone');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('true');
    });

    test('handles whitespace and newlines between arguments', () => {
        const source = `$toggleBtn(\n    'mute',\n    false\n).out();`;
        const span = findToggleBtnStateSpan(source, 'mute');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('false');
    });

    test('ignores a commented-out toggle with the same label', () => {
        const source =
            `// $toggleBtn('drone', true)\n` +
            `$toggleBtn('drone', false).out();\n`;
        const liveStateStart = source.lastIndexOf('false');
        const span = findToggleBtnStateSpan(source, 'drone');
        expect(span).toEqual({
            start: liveStateStart,
            end: liveStateStart + 'false'.length,
        });
    });

    test('ignores a toggle spelled inside a string literal', () => {
        const source =
            `const s = "$toggleBtn('drone', true)";\n` +
            `$toggleBtn('drone', false).out();\n`;
        const liveStateStart = source.lastIndexOf('false');
        const span = findToggleBtnStateSpan(source, 'drone');
        expect(span).toEqual({
            start: liveStateStart,
            end: liveStateStart + 'false'.length,
        });
    });

    test('skips comments between the comma and the state literal', () => {
        const source = `$toggleBtn('drone', /* start muted */ false).out();`;
        const span = findToggleBtnStateSpan(source, 'drone');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('false');
    });

    test('matches labels whose literals use escape sequences', () => {
        const source = `$toggleBtn("a\\\\b", false).out();`;
        const span = findToggleBtnStateSpan(source, 'a\\b');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('false');
    });

    test('matches labels containing both quote characters', () => {
        const source = `$toggleBtn('it\\'s "on"', true).out();`;
        const span = findToggleBtnStateSpan(source, `it's "on"`);
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('true');
    });

    test('returns null when the label is absent', () => {
        const source = `$toggleBtn('drone', false).out();`;
        expect(findToggleBtnStateSpan(source, 'nope')).toBeNull();
    });

    test('returns null for $btn calls', () => {
        const source = `$btn('play').out();`;
        expect(findToggleBtnStateSpan(source, 'play')).toBeNull();
    });
});
