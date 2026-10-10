import { describe, expect, test } from 'vitest';

import schemas from '@modular/core/schemas.json';

import { applyEdits } from '../migrationEdits';
import type { CodeStyle } from '../objectPropertyInsert';
import { computeControlQuickFixes } from '../controlQuickFix';
import type { ControlQuickFix } from '../controlQuickFix';

const LAYOUT: CodeStyle = {
    bracketSpacing: true,
    printWidth: 80,
    quote: "'",
    tabWidth: 2,
    trailingComma: true,
    useTabs: false,
};

/** Cursor marked with '|'; returns the source without the marker + offset. */
function fixture(text: string): { source: string; offset: number } {
    const offset = text.indexOf('|');
    expect(offset).toBeGreaterThanOrEqual(0);
    return { offset, source: text.slice(0, offset) + text.slice(offset + 1) };
}

function fixesAt(
    text: string,
    layout: CodeStyle = LAYOUT,
): { source: string; fixes: ControlQuickFix[] } {
    const { source, offset } = fixture(text);
    return {
        fixes: computeControlQuickFixes(source, offset, schemas, layout),
        source,
    };
}

function addFix(fixes: ControlQuickFix[], param: string): ControlQuickFix {
    const fix = fixes.find((f) => f.title === `Add slider for '${param}'`);
    expect(fix).toBeDefined();
    return fix!;
}

/** The `$slider(...)` call that ends at the caret after applying `fix`. */
function sliderBeforeCaret(source: string, fix: ControlQuickFix): string {
    const out = apply(source, fix);
    const at = fix.span.start + fix.caretOffset;
    return out.slice(out.lastIndexOf('$slider(', at), at);
}

function apply(source: string, fix: ControlQuickFix): string {
    const { source: out, conflict } = applyEdits(source, [
        { end: fix.span.end, replacement: fix.newText, start: fix.span.start },
    ]);
    expect(conflict).toBe(false);
    return out;
}

function wrapFix(fixes: ControlQuickFix[]): ControlQuickFix {
    const wraps = fixes.filter((f) => f.kind === 'wrap');
    expect(wraps).toHaveLength(1);
    return wraps[0];
}

function addTitles(fixes: ControlQuickFix[]): string[] {
    return fixes.filter((f) => f.kind === 'add').map((f) => f.title);
}

describe('computeControlQuickFixes — wrap in params object', () => {
    test('wraps a numeric literal with the schema range', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { shape: 2.|5 }).out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'shape' in $slider`);
        expect(fix.preferred).toBe(true);
        expect(apply(source, fix)).toBe(
            `$saw('a3', { shape: $slider('shape', 2.5, 0, 5) }).out();`,
        );
    });

    test('the caret lands just past the inserted call', () => {
        const { source, fixes } = fixesAt(`$saw('a3', { shape: 2.|5 }).out();`);
        expect(sliderBeforeCaret(source, wrapFix(fixes))).toBe(
            `$slider('shape', 2.5, 0, 5)`,
        );
    });

    test('cursor on the property name still offers the wrap', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { sha|pe: 2.5 }).out();`,
        );
        const fix = wrapFix(fixes);
        expect(apply(source, fix)).toContain(
            `{ shape: $slider('shape', 2.5, 0, 5) }`,
        );
    });

    test('quoted keys resolve like identifier keys', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { 'shape': 2.|5 }).out();`,
        );
        const fix = wrapFix(fixes);
        expect(apply(source, fix)).toContain(
            `{ 'shape': $slider('shape', 2.5, 0, 5) }`,
        );
    });
});

describe('computeControlQuickFixes — wrap chained sugar arguments', () => {
    test('.amplitude uses the $scaleAndShift scale range', () => {
        const { source, fixes } = fixesAt(
            `$sine('c4').amplitude(0.|3).out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'amplitude' in $slider`);
        expect(apply(source, fix)).toBe(
            `$sine('c4').amplitude($slider('amplitude', 0.3, 0, 10)).out();`,
        );
    });

    test('.amp labels with the method name as written', () => {
        const { source, fixes } = fixesAt(`$sine('c4').amp(0.|3).out();`);
        const fix = wrapFix(fixes);
        expect(apply(source, fix)).toContain(`$slider('amp', 0.3, 0, 10)`);
    });

    test('.gain narrows to the perceptual 0..5 range', () => {
        const { source, fixes } = fixesAt(`$sine('c4').gain(2.|5).out();`);
        const fix = wrapFix(fixes);
        expect(apply(source, fix)).toContain(`$slider('gain', 2.5, 0, 5)`);
    });

    test('sugar on a deep chain still resolves', () => {
        const { source, fixes } = fixesAt(
            `$sine(0).$.lpf('100hz').amplitude(0.|3).out();`,
        );
        const fix = wrapFix(fixes);
        expect(apply(source, fix)).toContain(
            `.amplitude($slider('amplitude', 0.3, 0, 10))`,
        );
    });
});

describe('computeControlQuickFixes — wrap .$./.$m. chain arguments', () => {
    test('.$. maps arguments past the injected signal', () => {
        const { source, fixes } = fixesAt(
            `$sine(0).$.lpf('100hz', 2.|5).out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'resonance' in $slider`);
        expect(apply(source, fix)).toContain(
            `.$.lpf('100hz', $slider('resonance', 2.5, 0, 5))`,
        );
    });

    test('.$m. shifts the mapping by the leading mix argument', () => {
        const { source, fixes } = fixesAt(
            `$sine(0).$m.lpf(2.5, '100hz', 1.|0).out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'resonance' in $slider`);
        expect(apply(source, fix)).toContain(
            `$slider('resonance', 1.0, 0, 5)`,
        );
    });

    test('.$m. mix argument wraps with the crossfade range', () => {
        const { source, fixes } = fixesAt(
            `$sine(0).$m.lpf(2.|5, '100hz').out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'mix' in $slider`);
        expect(apply(source, fix)).toContain(`$slider('mix', 2.5, 0, 5)`);
    });
});

describe('computeControlQuickFixes — namespaced modules', () => {
    test('direct dotted factory call resolves', () => {
        const { source, fixes } = fixesAt(
            `$unstable.filter.lp($sine(0), '100hz', 2.|5).out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'resonance' in $slider`);
        expect(apply(source, fix)).toContain(`$slider('resonance', 2.5, `);
    });

    test('dotted chain path resolves', () => {
        const { source, fixes } = fixesAt(
            `$sine(0).$.unstable.filter.lp('100hz', 2.|5).out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'resonance' in $slider`);
        expect(apply(source, fix)).toContain(`$slider('resonance', 2.5, `);
    });
});

describe('computeControlQuickFixes — direct positional arguments', () => {
    test('wraps a positional literal by its param name', () => {
        const { source, fixes } = fixesAt(
            `$lpf($sine(0), '100hz', 2.|5).out();`,
        );
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'resonance' in $slider`);
        expect(apply(source, fix)).toContain(
            `$lpf($sine(0), '100hz', $slider('resonance', 2.5, 0, 5))`,
        );
    });
});

describe('computeControlQuickFixes — add sliders for missing params', () => {
    test('offers unset signal params, not supplied ones', () => {
        const { fixes } = fixesAt(`$saw('a3'|).out();`);
        const titles = addTitles(fixes);
        expect(titles).toContain(`Add slider for 'shape'`);
        expect(titles).toContain(`Add slider for 'fm'`);
        expect(titles).toContain(`Add slider for 'sync'`);
        expect(titles).toContain(`Add slider for 'phaseOffset'`);
        expect(titles).not.toContain(`Add note slider for 'freq'`);
    });

    test('creates the config object when absent', () => {
        const { source, fixes } = fixesAt(`$saw('a3'|).out();`);
        const shape = fixes.find(
            (f) => f.title === `Add slider for 'shape'`,
        )!;
        expect(apply(source, shape)).toBe(
            `$saw('a3', { shape: $slider('shape', 0, 0, 5) }).out();`,
        );
        expect(sliderBeforeCaret(source, shape)).toBe(
            `$slider('shape', 0, 0, 5)`,
        );
    });

    test('appends to an existing config object and omits set keys', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { shape: 2.5| }).out();`,
        );
        const titles = addTitles(fixes);
        expect(titles).not.toContain(`Add slider for 'shape'`);
        const fm = fixes.find((f) => f.title === `Add slider for 'fm'`)!;
        expect(apply(source, fm)).toBe(
            `$saw('a3', { shape: 2.5, fm: $slider('fm', 0, -5, 5) }).out();`,
        );
    });

    test('inserts into an empty config object', () => {
        const { source, fixes } = fixesAt(`$saw('a3', {|}).out();`);
        const fm = fixes.find((f) => f.title === `Add slider for 'fm'`)!;
        expect(apply(source, fm)).toBe(
            `$saw('a3', { fm: $slider('fm', 0, -5, 5) }).out();`,
        );
    });

    test('fills the next open positional slot positionally', () => {
        const { source, fixes } = fixesAt(`$lpf($sine(0), '100hz'|).out();`);
        const res = fixes.find(
            (f) => f.title === `Add slider for 'resonance'`,
        )!;
        expect(apply(source, res)).toBe(
            `$lpf($sine(0), '100hz', $slider('resonance', 0, 0, 5)).out();`,
        );
    });

    test('omits params behind an unfilled positional slot', () => {
        const { fixes } = fixesAt(`$lpf($sine(0)|).out();`);
        const titles = addTitles(fixes);
        expect(titles).toContain(`Add hz slider for 'cutoff'`);
        expect(titles).not.toContain(`Add slider for 'resonance'`);
    });

    test('chains treat the injected signal as supplied', () => {
        const { fixes } = fixesAt(`$sine(0).$.lpf('100hz'|).out();`);
        const titles = addTitles(fixes);
        expect(titles).toContain(`Add slider for 'resonance'`);
        expect(titles).not.toContain(`Add slider for 'input'`);
        expect(titles).not.toContain(`Add hz slider for 'cutoff'`);
    });

    test('shorthand config properties count as supplied', () => {
        const { fixes } = fixesAt(
            `const shape = 1;\n$saw('a3', { shape| }).out();`,
        );
        expect(addTitles(fixes)).not.toContain(`Add slider for 'shape'`);
    });

    test('inner call wins over the outer one', () => {
        const { fixes } = fixesAt(`$lpf($sine(|0), '100hz').out();`);
        const titles = addTitles(fixes);
        expect(titles).toContain(`Add slider for 'fm'`);
        expect(titles).not.toContain(`Add hz slider for 'cutoff'`);
    });
});

describe('computeControlQuickFixes — labels', () => {
    test('dedupes against existing $slider labels', () => {
        const { source, fixes } = fixesAt(
            `const s = $slider('shape', 1, 0, 5);\n` +
                `$saw('a3', { shape: 2.|5, fm: s }).out();`,
        );
        const fix = wrapFix(fixes);
        expect(apply(source, fix)).toContain(
            `shape: $slider('shape 2', 2.5, 0, 5)`,
        );
    });

    test('dedupes against ungrouped buttons too', () => {
        const { source, fixes } = fixesAt(
            `const b = $btn('shape');\n$saw('a3', { shape: 2.|5, sync: b }).out();`,
        );
        expect(apply(source, wrapFix(fixes))).toContain(
            `shape: $slider('shape 2', 2.5, 0, 5)`,
        );
    });

    test('labels inside a group do not force dedupe', () => {
        const { source, fixes } = fixesAt(
            `const g = $cGroup('G');\nconst a = $slider('shape', 1, 0, 5, g);\n` +
                `$saw('a3', { shape: 2.|5, fm: a }).out();`,
        );
        expect(apply(source, wrapFix(fixes))).toContain(
            `shape: $slider('shape', 2.5, 0, 5)`,
        );
    });

    test('commented-out sliders do not force dedupe', () => {
        const { source, fixes } = fixesAt(
            `// $slider('shape', 1, 0, 5)\n` +
                `$saw('a3', { shape: 2.|5 }).out();`,
        );
        const fix = wrapFix(fixes);
        expect(apply(source, fix)).toContain(
            `shape: $slider('shape', 2.5, 0, 5)`,
        );
    });
});

describe('computeControlQuickFixes — out-of-range literals', () => {
    test('widens the max to include the value', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { shape: |7 }).out();`,
        );
        expect(apply(source, wrapFix(fixes))).toContain(
            `$slider('shape', 7, 0, 7)`,
        );
    });

    test('widens the min and preserves the sign', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { fm: -|6 }).out();`,
        );
        expect(apply(source, wrapFix(fixes))).toContain(
            `$slider('fm', -6, -6, 5)`,
        );
    });
});

describe('computeControlQuickFixes — non-eligible sites', () => {
    test('a value already wrapped in $slider offers no wrap', () => {
        const { fixes } = fixesAt(
            `$saw('a3', { shape: $slider('shape', 2.|5, 0, 5) }).out();`,
        );
        expect(fixes.filter((f) => f.kind === 'wrap')).toHaveLength(0);
        expect(addTitles(fixes)).toContain(`Add slider for 'fm'`);
    });

    test('an identifier value offers no wrap but adds remain', () => {
        const { fixes } = fixesAt(
            `const x = 1;\n$saw('a3', { shape: x| }).out();`,
        );
        expect(fixes.filter((f) => f.kind === 'wrap')).toHaveLength(0);
        expect(addTitles(fixes)).toContain(`Add slider for 'fm'`);
    });

    test('unknown callees produce nothing', () => {
        expect(fixesAt(`foo(1.|5);`).fixes).toEqual([]);
        expect(fixesAt(`Math.max(1.|5);`).fixes).toEqual([]);
    });

    test('sugar on a non-DSL root produces nothing', () => {
        expect(fixesAt(`'str'.amplitude(0.|3);`).fixes).toEqual([]);
    });

    test('a non-signal config key offers no wrap', () => {
        const { fixes } = fixesAt(
            `$saw('a3', { id: |3 }).out();`,
        );
        expect(fixes.filter((f) => f.kind === 'wrap')).toHaveLength(0);
    });

    test('cursor outside any call produces nothing', () => {
        expect(fixesAt(`const x = 1;|\n$saw('a3').out();`).fixes).toEqual([]);
    });

    test('broken source does not throw', () => {
        expect(() =>
            fixesAt(`$saw('a3', { shape: 2.|5 `),
        ).not.toThrow();
    });
});

describe('computeControlQuickFixes — add-param layout', () => {
    test('expanded object gets the property on its own line', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', {\n  shape: 2,\n  sync: 4|\n}).out();`,
        );
        const fix = addFix(fixes, 'fm');
        expect(apply(source, fix)).toBe(
            `$saw('a3', {\n  shape: 2,\n  sync: 4,\n  fm: $slider('fm', 0, -5, 5)\n}).out();`,
        );
        expect(sliderBeforeCaret(source, fix)).toBe(`$slider('fm', 0, -5, 5)`);
    });

    test('expanded object keeps its trailing-comma style', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', {\n  shape: 2,\n  sync: 4,|\n}).out();`,
        );
        expect(apply(source, addFix(fixes, 'fm'))).toBe(
            `$saw('a3', {\n  shape: 2,\n  sync: 4,\n  fm: $slider('fm', 0, -5, 5),\n}).out();`,
        );
    });

    test('expanded object matches the existing indentation', () => {
        const { source, fixes } = fixesAt(
            `function f() {\n    $saw('a3', {\n        shape: 2,|\n    }).out();\n}`,
        );
        expect(apply(source, addFix(fixes, 'fm'))).toBe(
            `function f() {\n    $saw('a3', {\n        shape: 2,\n        fm: $slider('fm', 0, -5, 5),\n    }).out();\n}`,
        );
    });

    test('a line comment stays on the property it follows', () => {
        const withComma = fixesAt(
            `$saw('a3', {\n  sync: 4, // note|\n}).out();`,
        );
        expect(apply(withComma.source, addFix(withComma.fixes, 'fm'))).toBe(
            `$saw('a3', {\n  sync: 4, // note\n  fm: $slider('fm', 0, -5, 5),\n}).out();`,
        );
        const without = fixesAt(
            `$saw('a3', {\n  sync: 4 // note|\n}).out();`,
        );
        expect(apply(without.source, addFix(without.fixes, 'fm'))).toBe(
            `$saw('a3', {\n  sync: 4, // note\n  fm: $slider('fm', 0, -5, 5)\n}).out();`,
        );
    });

    test('single-line object stays inline when it fits', () => {
        const { source, fixes } = fixesAt(`$saw('a3', { shape: 2.5| }).out();`);
        expect(apply(source, addFix(fixes, 'fm'))).toBe(
            `$saw('a3', { shape: 2.5, fm: $slider('fm', 0, -5, 5) }).out();`,
        );
    });

    test('single-line object breaks when it would exceed printWidth', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { shape: 2.5| }).out();`,
            { ...LAYOUT, printWidth: 60 },
        );
        const fix = addFix(fixes, 'phaseOffset');
        expect(apply(source, fix)).toBe(
            `$saw('a3', {\n  shape: 2.5,\n  phaseOffset: $slider('phaseOffset', 0, 0, 1),\n}).out();`,
        );
        expect(sliderBeforeCaret(source, fix)).toBe(
            `$slider('phaseOffset', 0, 0, 1)`,
        );
    });

    test('breaking honors trailingComma none, tabs, and line indent', () => {
        const { source, fixes } = fixesAt(
            `\tconst s = $saw('a3', { shape: 2.5| })`,
            { ...LAYOUT, printWidth: 40, trailingComma: false, useTabs: true },
        );
        expect(apply(source, addFix(fixes, 'fm'))).toBe(
            `\tconst s = $saw('a3', {\n\t\tshape: 2.5,\n\t\tfm: $slider('fm', 0, -5, 5)\n\t})`,
        );
    });

    test('caret lands after the new slider when existing sliders are rewritten', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { shape: $slider('s', 1, 0, 5)| }).out();`,
            { ...LAYOUT, printWidth: 60 },
        );
        const fix = addFix(fixes, 'phaseOffset');
        expect(apply(source, fix)).toContain(`\n  shape: $slider('s', 1, 0, 5),\n`);
        expect(sliderBeforeCaret(source, fix)).toBe(
            `$slider('phaseOffset', 0, 0, 1)`,
        );
    });

    test('a single-line object with comments is not rebuilt', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', { shape: 2.5 /* x */| }).out();`,
            { ...LAYOUT, printWidth: 40 },
        );
        expect(apply(source, addFix(fixes, 'fm'))).toBe(
            `$saw('a3', { shape: 2.5, fm: $slider('fm', 0, -5, 5) /* x */ }).out();`,
        );
    });

    test('a new config object hugs the call when too wide', () => {
        const { source, fixes } = fixesAt(`$saw('a3'|).out();`, {
            ...LAYOUT,
            printWidth: 40,
        });
        const fix = addFix(fixes, 'phaseOffset');
        expect(apply(source, fix)).toBe(
            `$saw('a3', {\n  phaseOffset: $slider('phaseOffset', 0, 0, 1),\n}).out();`,
        );
        expect(sliderBeforeCaret(source, fix)).toBe(
            `$slider('phaseOffset', 0, 0, 1)`,
        );
    });
});

describe('computeControlQuickFixes — controls by signal type', () => {
    test('a note literal on a pitch param wraps in a note slider', () => {
        const { source, fixes } = fixesAt(`$saw('a|3').out();`);
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'freq' in a note $slider`);
        expect(fix.preferred).toBe(true);
        expect(apply(source, fix)).toBe(
            `$saw($slider('freq', 'a3', 'a1', 'a5')).out();`,
        );
        expect(sliderBeforeCaret(source, fix)).toBe(
            `$slider('freq', 'a3', 'a1', 'a5')`,
        );
    });

    test('an hz literal on a frequency param wraps in an hz slider', () => {
        const { source, fixes } = fixesAt(`$lpf($saw('a3'), '1000|hz').out();`);
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Wrap 'cutoff' in an hz $slider`);
        expect(apply(source, fix)).toBe(
            `$lpf($saw('a3'), $slider('cutoff', '1000hz', '250hz', '4000hz')).out();`,
        );
    });

    test('a literal keeps its unit and text; generated strings use the configured quote', () => {
        const { source, fixes } = fixesAt(`$saw("440|hz").out();`);
        expect(apply(source, wrapFix(fixes))).toBe(
            `$saw($slider('freq', "440hz", '110hz', '1760hz')).out();`,
        );
    });

    test('double-quote style applies to every generated string', () => {
        const dq = { ...LAYOUT, quote: '"' as const };
        const wrap = fixesAt(`$saw('a|3').out();`, dq);
        expect(apply(wrap.source, wrapFix(wrap.fixes))).toBe(
            `$saw($slider("freq", 'a3', "a1", "a5")).out();`,
        );
        const add = fixesAt(`$lpf($saw('a3')|).out();`, dq);
        const cutoff = add.fixes.find(
            (f) => f.title === `Add hz slider for 'cutoff'`,
        )!;
        expect(apply(add.source, cutoff)).toBe(
            `$lpf($saw('a3'), $slider("cutoff", "260hz", "65hz", "1040hz")).out();`,
        );
        const gate = fixesAt(`$adsr(|).out();`, dq);
        expect(gate.fixes.map((f) => f.newText)).toEqual([
            `$btn("gate")`,
            `$toggleBtn("gate", false)`,
        ]);
    });

    test('the two-octave span is clipped to the schema range', () => {
        // c9 is the top of the ±5 V range.
        const { source, fixes } = fixesAt(`$saw('b|8').out();`);
        expect(apply(source, wrapFix(fixes))).toBe(
            `$saw($slider('freq', 'b8', 'b6', 'c9')).out();`,
        );
    });

    test('a value past a schema limit keeps its full span on both sides', () => {
        // 1hz is ~8 octaves below C4, under the -5 V schema minimum.
        const low = fixesAt(`$saw('1|hz').out();`);
        expect(apply(low.source, wrapFix(low.fixes))).toBe(
            `$saw($slider('freq', '1hz', '0.25hz', '4hz')).out();`,
        );
        // c10 is above the +5 V (c9) schema maximum.
        const high = fixesAt(`$saw('c1|0').out();`);
        expect(apply(high.source, wrapFix(high.fixes))).toBe(
            `$saw($slider('freq', 'c10', 'c8', 'c12')).out();`,
        );
    });

    test('a string that is not a single note or hz value is not wrapped', () => {
        const { fixes } = fixesAt(`$saw('a3 c|4').out();`);
        expect(fixes.filter((f) => f.kind === 'wrap')).toEqual([]);
    });

    test('a number on a pitch param stays a volts slider', () => {
        const { source, fixes } = fixesAt(`$saw(0.|5).out();`);
        expect(apply(source, wrapFix(fixes))).toBe(
            `$saw($slider('freq', 0.5, -5, 5)).out();`,
        );
    });

    test('a number on a gate param offers a toggle, then a button', () => {
        const { source, fixes } = fixesAt(`$adsr(|5).out();`);
        expect(fixes.filter((f) => f.kind === 'wrap').map((f) => f.title)).toEqual([
            `Replace 'gate' with $toggleBtn`,
            `Replace 'gate' with $btn`,
        ]);
        const [toggle, button] = fixes;
        expect(toggle.preferred).toBe(true);
        expect(button.preferred).toBe(false);
        expect(apply(source, toggle)).toBe(`$adsr($toggleBtn('gate', true)).out();`);
        expect(apply(source, button)).toBe(`$adsr($btn('gate')).out();`);
        const low = fixesAt(`$adsr(|0).out();`);
        expect(apply(low.source, low.fixes[0])).toBe(
            `$adsr($toggleBtn('gate', false)).out();`,
        );
    });

    test('a number on a trig param offers a button', () => {
        const { source, fixes } = fixesAt(`$perc(|5).out();`);
        const fix = wrapFix(fixes);
        expect(fix.title).toBe(`Replace 'trigger' with $btn`);
        expect(apply(source, fix)).toBe(`$perc($btn('trigger')).out();`);
    });

    test('a missing pitch param adds a note slider around the default', () => {
        const { source, fixes } = fixesAt(`$saw(|).out();`);
        const fix = fixes.find((f) => f.title === `Add note slider for 'freq'`)!;
        expect(apply(source, fix)).toBe(
            `$saw($slider('freq', 'c4', 'c2', 'c6')).out();`,
        );
    });

    test('a missing frequency param adds an hz slider with a short default', () => {
        const { source, fixes } = fixesAt(`$lpf($saw('a3')|).out();`);
        const fix = fixes.find((f) => f.title === `Add hz slider for 'cutoff'`)!;
        expect(apply(source, fix)).toBe(
            `$lpf($saw('a3'), $slider('cutoff', '260hz', '65hz', '1040hz')).out();`,
        );
    });

    test('a missing gate param adds a button or a toggle', () => {
        const { source, fixes } = fixesAt(`$adsr(|).out();`);
        expect(addTitles(fixes).slice(0, 2)).toEqual([
            `Add button for 'gate'`,
            `Add toggle for 'gate'`,
        ]);
        expect(apply(source, fixes[0])).toBe(`$adsr($btn('gate')).out();`);
        expect(apply(source, fixes[1])).toBe(
            `$adsr($toggleBtn('gate', false)).out();`,
        );
        // The caret lands just past the inserted button call.
        const out = apply(source, fixes[0]);
        expect(
            out.slice(0, fixes[0].span.start + fixes[0].caretOffset),
        ).toBe(`$adsr($btn('gate')`);
    });
});

describe('computeControlQuickFixes — caret at a literal edge', () => {
    test('a caret just past a literal still wraps it', () => {
        for (const [text, expected] of [
            [`$saw('a3', { shape: 2.5| }).out();`, `$slider('shape', 2.5, 0, 5)`],
            [`$saw('a3', { fm: -6| }).out();`, `$slider('fm', -6, -6, 5)`],
            [`$lpf($saw('a3'), '100hz', 2.5|).out();`, `$slider('resonance', 2.5, 0, 5)`],
            [`$saw('a3'|).out();`, `$slider('freq', 'a3', 'a1', 'a5')`],
            [`$sine('c4').amplitude(0.3|).out();`, `$slider('amplitude', 0.3, 0, 10)`],
        ]) {
            const { source, fixes } = fixesAt(text);
            const fix = wrapFix(fixes);
            expect(fix.preferred).toBe(true);
            expect(apply(source, fix)).toContain(expected);
        }
    });

    test('a caret just before a literal still wraps it', () => {
        const { source, fixes } = fixesAt(`$saw('a3', { shape: |2.5 }).out();`);
        expect(apply(source, wrapFix(fixes))).toContain(
            `$slider('shape', 2.5, 0, 5)`,
        );
    });

    test('a caret past an inner call adds to the call being edited', () => {
        const { fixes } = fixesAt(`$lpf($sine(0)|).out();`);
        expect(addTitles(fixes)).toContain(`Add hz slider for 'cutoff'`);
        expect(addTitles(fixes)).not.toContain(`Add slider for 'fm'`);
    });
});

describe('computeControlQuickFixes — literal and layout edge cases', () => {
    test('a negated literal keeps its text and reads its value', () => {
        const { source, fixes } = fixesAt(`$saw('a3', { fm: -1_0|00 }).out();`);
        expect(apply(source, wrapFix(fixes))).toContain(
            `$slider('fm', -1_000, -1000, 5)`,
        );
        const hex = fixesAt(`$saw('a3', { fm: -0x|2 }).out();`);
        expect(apply(hex.source, wrapFix(hex.fixes))).toContain(
            `$slider('fm', -0x2, -5, 5)`,
        );
    });

    test('a unary plus literal is not wrapped', () => {
        const { fixes } = fixesAt(`$saw('a3', { shape: +|3 }).out();`);
        expect(fixes.filter((f) => f.kind === 'wrap')).toEqual([]);
    });

    test('a comment before the trailing comma is kept, not doubled', () => {
        const { source, fixes } = fixesAt(
            `$saw('a3', {\n  shape: 1 /* x */,|\n}).out();`,
        );
        expect(apply(source, addFix(fixes, 'fm'))).toBe(
            `$saw('a3', {\n  shape: 1 /* x */,\n  fm: $slider('fm', 0, -5, 5),\n}).out();`,
        );
    });

    test('a JS global method is not mistaken for a sugar method', () => {
        expect(fixesAt(`Math.exp(1|);`).fixes).toEqual([]);
        expect(fixesAt(`JSON.$.lpf('100hz', 2|);`).fixes).toEqual([]);
    });

    test('sugar on a const the patch declares still resolves', () => {
        const { source, fixes } = fixesAt(
            `const osc = $saw('a3');\nosc.amplitude(0.|3).out();`,
        );
        expect(apply(source, wrapFix(fixes))).toContain(
            `osc.amplitude($slider('amplitude', 0.3, 0, 10))`,
        );
    });

    test('CRLF sources get CRLF inserts with an exact caret', () => {
        const expanded = fixesAt(
            `$saw('a3', {\r\n  shape: 2 // note|\r\n}).out();`,
        );
        const fm = addFix(expanded.fixes, 'fm');
        expect(apply(expanded.source, fm)).toBe(
            `$saw('a3', {\r\n  shape: 2, // note\r\n  fm: $slider('fm', 0, -5, 5)\r\n}).out();`,
        );
        expect(sliderBeforeCaret(expanded.source, fm)).toBe(
            `$slider('fm', 0, -5, 5)`,
        );

        const wide = fixesAt(`$saw('a3', { shape: 2.5| }).out();\r\n`, {
            ...LAYOUT,
            printWidth: 60,
        });
        const po = addFix(wide.fixes, 'phaseOffset');
        expect(apply(wide.source, po)).toBe(
            `$saw('a3', {\r\n  shape: 2.5,\r\n  phaseOffset: $slider('phaseOffset', 0, 0, 1),\r\n}).out();\r\n`,
        );
        expect(sliderBeforeCaret(wide.source, po)).toBe(
            `$slider('phaseOffset', 0, 0, 1)`,
        );
    });
});
