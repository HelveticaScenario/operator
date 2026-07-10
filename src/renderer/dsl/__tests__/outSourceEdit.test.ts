import { describe, expect, test } from 'vitest';

import {
    computeOutNumericOptionEdit,
    computeOutOptionEdit,
    computeSetOutputGainEdit,
} from '../outSourceEdit';
import type { OutOptionProp } from '../outSourceEdit';

/** Apply an edit and return the resulting source, or null when no edit. */
function apply(
    source: string,
    anchor: number,
    prop: OutOptionProp,
    value: boolean,
): string | null {
    const edit = computeOutOptionEdit(source, anchor, prop, value);
    if (!edit) {
        return null;
    }
    return source.slice(0, edit.start) + edit.text + source.slice(edit.end);
}

/** Anchor at the method name of the first `.out(` / `.outMono(` call. */
function anchorOf(source: string, method: 'out' | 'outMono'): number {
    const idx = source.indexOf(`.${method}(`);
    expect(idx).toBeGreaterThanOrEqual(0);
    return idx + 1;
}

describe('computeOutOptionEdit — set on .out', () => {
    test('bare call gains an options object', () => {
        const source = `$sine('c4').out()`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBe(
            `$sine('c4').out({ mute: true })`,
        );
    });

    test('existing object gains the property before the brace', () => {
        const source = `$sine('c4').out({ gain: 2 })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBe(
            `$sine('c4').out({ gain: 2, mute: true })`,
        );
    });

    test('existing object with trailing comma', () => {
        const source = `$sine('c4').out({ gain: 2, })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBe(
            `$sine('c4').out({ gain: 2, mute: true })`,
        );
    });

    test('empty object literal is replaced', () => {
        const source = `$sine('c4').out({})`;
        expect(apply(source, anchorOf(source, 'out'), 'solo', true)).toBe(
            `$sine('c4').out({ solo: true })`,
        );
    });

    test('false literal flips to true', () => {
        const source = `$sine('c4').out({ mute: false })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBe(
            `$sine('c4').out({ mute: true })`,
        );
    });

    test('non-literal value returns null', () => {
        const source = `$sine('c4').out({ mute: someVar })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBeNull();
    });

    test('multi-line call inserts correctly', () => {
        const source = `$sine('c4').out({\n    gain: 2,\n    label: 'lead',\n})`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBe(
            `$sine('c4').out({\n    gain: 2,\n    label: 'lead', mute: true })`,
        );
    });
});

describe('computeOutOptionEdit — set on .outMono', () => {
    test('channel-only call appends an options object', () => {
        const source = `$saw('c2').outMono(0)`;
        expect(apply(source, anchorOf(source, 'outMono'), 'solo', true)).toBe(
            `$saw('c2').outMono(0, { solo: true })`,
        );
    });

    test('bare call supplies the default channel', () => {
        const source = `$saw('c2').outMono()`;
        expect(apply(source, anchorOf(source, 'outMono'), 'mute', true)).toBe(
            `$saw('c2').outMono(0, { mute: true })`,
        );
    });

    test('positional gain is wrapped into the object', () => {
        const source = `$saw('c2').outMono(0, 2.5)`;
        expect(apply(source, anchorOf(source, 'outMono'), 'mute', true)).toBe(
            `$saw('c2').outMono(0, { gain: 2.5, mute: true })`,
        );
    });

    test('signal-expression gain is wrapped verbatim', () => {
        const source = `$saw('c2').outMono(0, $sine('1hz').range(0, 5))`;
        expect(apply(source, anchorOf(source, 'outMono'), 'mute', true)).toBe(
            `$saw('c2').outMono(0, { gain: $sine('1hz').range(0, 5), mute: true })`,
        );
    });

    test('existing options object gains the property', () => {
        const source = `$saw('c2').outMono(0, { gain: 2.5, label: 'bass' })`;
        expect(apply(source, anchorOf(source, 'outMono'), 'mute', true)).toBe(
            `$saw('c2').outMono(0, { gain: 2.5, label: 'bass', mute: true })`,
        );
    });
});

describe('computeOutOptionEdit — remove', () => {
    test('sole property removes the whole object', () => {
        const source = `$sine('c4').out({ mute: true })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', false)).toBe(
            `$sine('c4').out()`,
        );
    });

    test('leading property removal keeps the rest', () => {
        const source = `$sine('c4').out({ mute: true, label: 'x' })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', false)).toBe(
            `$sine('c4').out({ label: 'x' })`,
        );
    });

    test('trailing property removal keeps the rest', () => {
        const source = `$sine('c4').out({ label: 'x', mute: true })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', false)).toBe(
            `$sine('c4').out({ label: 'x' })`,
        );
    });

    test('outMono sole property removes object and comma', () => {
        const source = `$saw('c2').outMono(0, { solo: true })`;
        expect(apply(source, anchorOf(source, 'outMono'), 'solo', false)).toBe(
            `$saw('c2').outMono(0)`,
        );
    });

    test('outMono keeps the object form when gain remains', () => {
        const source = `$saw('c2').outMono(0, { gain: 2.5, mute: true })`;
        expect(apply(source, anchorOf(source, 'outMono'), 'mute', false)).toBe(
            `$saw('c2').outMono(0, { gain: 2.5 })`,
        );
    });

    test('missing property returns null', () => {
        const source = `$sine('c4').out({ gain: 2 })`;
        expect(
            apply(source, anchorOf(source, 'out'), 'mute', false),
        ).toBeNull();
    });

    test('non-literal value returns null', () => {
        const source = `$sine('c4').out({ mute: isMuted })`;
        expect(
            apply(source, anchorOf(source, 'out'), 'mute', false),
        ).toBeNull();
    });
});

describe('computeOutNumericOptionEdit — pan', () => {
    function applyNumeric(
        source: string,
        anchor: number,
        prop: string,
        value: number | null,
    ): string | null {
        const edit = computeOutNumericOptionEdit(source, anchor, prop, value);
        if (!edit) {
            return null;
        }
        return (
            source.slice(0, edit.start) + edit.text + source.slice(edit.end)
        );
    }

    test('bare call gains a pan option', () => {
        const source = `$sine('c4').out()`;
        expect(
            applyNumeric(source, anchorOf(source, 'out'), 'pan', -2.5),
        ).toBe(`$sine('c4').out({ pan: -2.5 })`);
    });

    test('existing numeric pan is replaced', () => {
        const source = `$sine('c4').out({ pan: 3, label: 'x' })`;
        expect(
            applyNumeric(source, anchorOf(source, 'out'), 'pan', -1.2),
        ).toBe(`$sine('c4').out({ pan: -1.2, label: 'x' })`);
    });

    test('null removes the pan property', () => {
        const source = `$sine('c4').out({ pan: 3, label: 'x' })`;
        expect(
            applyNumeric(source, anchorOf(source, 'out'), 'pan', null),
        ).toBe(`$sine('c4').out({ label: 'x' })`);
    });

    test('sole pan property removal drops the object', () => {
        const source = `$sine('c4').out({ pan: 3 })`;
        expect(
            applyNumeric(source, anchorOf(source, 'out'), 'pan', null),
        ).toBe(`$sine('c4').out()`);
    });

    test('a signal-valued pan is not edited', () => {
        const source = `$sine('c4').out({ pan: $sine('1hz') })`;
        expect(
            applyNumeric(source, anchorOf(source, 'out'), 'pan', 2),
        ).toBeNull();
    });

    test('drag-noise precision is trimmed', () => {
        const source = `$sine('c4').out()`;
        expect(
            applyNumeric(
                source,
                anchorOf(source, 'out'),
                'pan',
                -1.2000000000000002,
            ),
        ).toBe(`$sine('c4').out({ pan: -1.2 })`);
    });

    test('gain on outMono keeps the positional form', () => {
        const source = `$saw('c2').outMono(0, 2.5)`;
        expect(
            applyNumeric(source, anchorOf(source, 'outMono'), 'gain', 3.1),
        ).toBe(`$saw('c2').outMono(0, 3.1)`);
    });

    test('gain on channel-only outMono appends positionally', () => {
        const source = `$saw('c2').outMono(0)`;
        expect(
            applyNumeric(source, anchorOf(source, 'outMono'), 'gain', 3.1),
        ).toBe(`$saw('c2').outMono(0, 3.1)`);
    });

    test('gain removal restores the bare outMono call', () => {
        const source = `$saw('c2').outMono(0, 2.5)`;
        expect(
            applyNumeric(source, anchorOf(source, 'outMono'), 'gain', null),
        ).toBe(`$saw('c2').outMono(0)`);
    });

    test('a signal-valued positional gain is not edited', () => {
        const source = `$saw('c2').outMono(0, $sine('1hz'))`;
        expect(
            applyNumeric(source, anchorOf(source, 'outMono'), 'gain', 3),
        ).toBeNull();
    });

    test('gain in an options object edits the property', () => {
        const source = `$saw('c2').outMono(0, { gain: 2.5, label: 'bass' })`;
        expect(
            applyNumeric(source, anchorOf(source, 'outMono'), 'gain', 3.1),
        ).toBe(`$saw('c2').outMono(0, { gain: 3.1, label: 'bass' })`);
    });
});

describe('computeSetOutputGainEdit — master fader', () => {
    function applyMaster(source: string, value: number): string | null {
        const edit = computeSetOutputGainEdit(source, value);
        if (!edit) {
            return null;
        }
        return (
            source.slice(0, edit.start) + edit.text + source.slice(edit.end)
        );
    }

    test('updates an existing numeric call', () => {
        const source = `$setOutputGain(2.5)\n$sine('c4').out()`;
        expect(applyMaster(source, 4)).toBe(
            `$setOutputGain(4)\n$sine('c4').out()`,
        );
    });

    test('appends a call when none exists', () => {
        const source = `$sine('c4').out()`;
        expect(applyMaster(source, 3.2)).toBe(
            `$sine('c4').out()\n$setOutputGain(3.2)\n`,
        );
    });

    test('a signal-driven output gain is not edited', () => {
        const source = `$setOutputGain($sine('1hz'))\n$sine('c4').out()`;
        expect(applyMaster(source, 4)).toBeNull();
    });

    test('a commented-out call does not count', () => {
        const source = `// $setOutputGain(1)\n$sine('c4').out()\n`;
        expect(applyMaster(source, 4)).toBe(
            `// $setOutputGain(1)\n$sine('c4').out()\n$setOutputGain(4)\n`,
        );
    });
});

describe('computeOutOptionEdit — robustness', () => {
    test('anchor not at an out call returns null', () => {
        const source = `$sine('c4').scope()`;
        expect(apply(source, source.indexOf('scope'), 'mute', true)).toBeNull();
    });

    test('braces inside string values do not confuse parsing', () => {
        const source = `$sine('c4').out({ label: 'a } b', gain: 2 })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBe(
            `$sine('c4').out({ label: 'a } b', gain: 2, mute: true })`,
        );
    });

    test('a property named in a comment is not matched', () => {
        const source = `$sine('c4').out({ /* mute: true */ gain: 2 })`;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBe(
            `$sine('c4').out({ /* mute: true */ gain: 2, mute: true })`,
        );
    });

    test('outMono anchor distinguishes from out', () => {
        const source = `$saw('c2').outMono(0, 2.5)`;
        const anchor = anchorOf(source, 'outMono');
        expect(apply(source, anchor, 'solo', true)).toBe(
            `$saw('c2').outMono(0, { gain: 2.5, solo: true })`,
        );
    });

    test('nested call in another arg position stays untouched', () => {
        const source = `$mix([$sine('c4'), $saw('e4')]).out({ pan: -2 })`;
        expect(apply(source, anchorOf(source, 'out'), 'solo', true)).toBe(
            `$mix([$sine('c4'), $saw('e4')]).out({ pan: -2, solo: true })`,
        );
    });

    test('unbalanced call returns null', () => {
        const source = `$sine('c4').out({ gain: 2 `;
        expect(apply(source, anchorOf(source, 'out'), 'mute', true)).toBeNull();
    });
});
