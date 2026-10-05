import { describe, expect, test } from 'vitest';

import { findSliderValueSpan } from '../sliderSourceEdit';

describe('findSliderValueSpan', () => {
    test('locates the value literal of a single slider', () => {
        const source = `$sine('a3', {fm: $slider('x', 2.5, 0, 5)}).out();`;
        const span = findSliderValueSpan(source, 'x');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('2.5');
    });

    test('ignores a commented-out slider with the same label', () => {
        // The commented call appears first in the text; the live call later.
        const source =
            `// const x = $slider('x', 3.045,0,1)\n` +
            `\n` +
            `$sine('a3', {fm: $slider('x', 0,0,5)}).out();\n`;
        const liveValueStart = source.indexOf('0,0,5)');
        const span = findSliderValueSpan(source, 'x');
        expect(span).toEqual({
            start: liveValueStart,
            end: liveValueStart + 1,
        });
    });

    test('ignores a slider inside a block comment', () => {
        const source =
            `/* $slider('g', 9, 0, 10) */\n` +
            `$sine('a3', {fm: $slider('g', 4, 0, 10)}).out();\n`;
        const liveValueStart = source.indexOf('4, 0, 10)');
        const span = findSliderValueSpan(source, 'g');
        expect(span).toEqual({
            start: liveValueStart,
            end: liveValueStart + 1,
        });
    });

    test('does not treat // inside a string as a comment', () => {
        // A `//` inside a string literal must not hide the real slider that
        // textually follows it.
        const source =
            `const url = "http://$slider('x', 7, 0, 9)";\n` +
            `$sine('a3', {fm: $slider('x', 1, 0, 9)}).out();\n`;
        const liveValueStart = source.indexOf('1, 0, 9)');
        const span = findSliderValueSpan(source, 'x');
        expect(span).toEqual({
            start: liveValueStart,
            end: liveValueStart + 1,
        });
    });

    test('skips comments between the comma and the value literal', () => {
        const source = `$sine($slider('x', /* volts */ 2.5, 0, 5)).out();`;
        const span = findSliderValueSpan(source, 'x');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('2.5');
    });

    test('matches labels whose literals use escape sequences', () => {
        const source = `$sine($slider("cut\\noff", 1, 0, 5)).out();`;
        const span = findSliderValueSpan(source, 'cut\noff');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('1');
    });

    test('returns null when the label is absent', () => {
        const source = `$sine('a3', {fm: $slider('x', 0, 0, 5)}).out();`;
        expect(findSliderValueSpan(source, 'nope')).toBeNull();
    });

    test('locates a double-quoted hz string value including quotes', () => {
        const source = `$sine($slider("Pitch", "440hz", "55hz", "1760hz")).out();`;
        const span = findSliderValueSpan(source, 'Pitch');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe('"440hz"');
    });

    test('locates a single-quoted note string value including quotes', () => {
        const source = `$saw($slider('Root', 'a#3', 'c2', 'c5')).out();`;
        const span = findSliderValueSpan(source, 'Root');
        expect(span).not.toBeNull();
        expect(source.slice(span!.start, span!.end)).toBe("'a#3'");
    });

    test('ignores a commented-out string-valued slider with the same label', () => {
        const source =
            `// $slider('p', '880hz', '55hz', '1760hz')\n` +
            `$sine($slider('p', '440hz', '55hz', '1760hz')).out();\n`;
        const liveValueStart = source.indexOf(`'440hz'`);
        const span = findSliderValueSpan(source, 'p');
        expect(span).toEqual({
            start: liveValueStart,
            end: liveValueStart + `'440hz'`.length,
        });
    });
});
