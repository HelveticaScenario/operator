import { describe, expect, test } from 'vitest';

import { extractControls, resolveControls } from '../extractControls';

describe('extractControls', () => {
    test('reads sliders and buttons from literal calls, in source order', () => {
        const { sliders, buttons } = extractControls(`
            const vol = $slider('vol', 0.5, 0, 1);
            $sine('c4').amplitude(vol).out();
            $btn("hit");
            $toggleBtn('drone', true);
            $slider("off", -1, -2, 2);
        `);
        expect(sliders).toMatchObject([
            { label: 'vol', max: 1, min: 0, unit: 'number', value: 0.5 },
            { label: 'off', max: 2, min: -2, unit: 'number', value: -1 },
        ]);
        expect(buttons).toMatchObject([
            { label: 'hit', mode: 'gate', value: false },
            { label: 'drone', mode: 'toggle', value: true },
        ]);
    });

    test('records each call site: the callee and just inside the paren', () => {
        const source = `const a = 1;\n$slider( 'x', 1, 0, 2);\n$btn('b');`;
        const { sliders, buttons } = extractControls(source);
        expect(sliders[0].callStart).toBe(source.indexOf('$slider'));
        expect(sliders[0].argsStart).toBe(source.indexOf('$slider(') + 8);
        expect(buttons[0].callStart).toBe(source.indexOf('$btn'));
        expect(buttons[0].argsStart).toBe(source.indexOf('$btn(') + 5);
    });

    test('converts hz and note sliders to V/Oct volts', () => {
        const { sliders } = extractControls(
            `$slider('root', 'c4', 'c2', 'c6'); $slider('f', '261.6255653005986hz', '55hz', '880hz');`,
        );
        expect(sliders[0]).toMatchObject({ unit: 'note', value: 0, min: -2 });
        expect(sliders[1]).toMatchObject({ unit: 'hz' });
        expect((sliders[1] as { value: number }).value).toBeCloseTo(0, 10);
    });

    test('finds controls nested inside other calls and functions', () => {
        const { sliders, buttons } = extractControls(
            `const voice = () => $saw($slider('p', 'a3', 'c2', 'c6')).amp($btn('gate'));`,
        );
        expect(sliders.map((s) => s.label)).toEqual(['p']);
        expect(buttons.map((b) => b.label)).toEqual(['gate']);
    });

    test('ignores controls in comments and strings', () => {
        const { sliders } = extractControls(`
            // $slider('a', 1, 0, 2)
            /* $slider('b', 1, 0, 2) */
            const s = "$slider('c', 1, 0, 2)";
        `);
        expect(sliders).toEqual([]);
    });

    test('reports labelled calls that evaluation would reject as incomplete', () => {
        const { sliders, buttons } = extractControls(`
            $slider('nonLiteral', x, 0, 1);
            $slider('arity', 1, 0);
            $slider('mixed', '440hz', 0, 5);
            $slider('backwards', 1, 2, 0);
            $slider('garbage', 'h4', 'c2', 'c6');
            $btn('extra', 1);
            $toggleBtn('noState');
            $toggleBtn('numeric', 1);
        `);
        expect(sliders.every((s) => 'incomplete' in s)).toBe(true);
        expect(sliders.map((s) => s.label)).toEqual([
            'nonLiteral',
            'arity',
            'mixed',
            'backwards',
            'garbage',
        ]);
        expect(buttons.every((b) => 'incomplete' in b)).toBe(true);
        expect(buttons).toHaveLength(3);
    });

    test('omits calls without a literal label', () => {
        const { sliders } = extractControls(`$slider(name, 1, 0, 2);`);
        expect(sliders).toEqual([]);
    });

    test('keeps only the first control for a duplicated label', () => {
        const { sliders, buttons } = extractControls(`
            $slider('x', 1, 0, 2);
            $slider('x', 2, 0, 2);
            $btn('x');
        `);
        expect(sliders).toHaveLength(1);
        expect(sliders[0]).toMatchObject({ value: 1 });
        expect(buttons).toEqual([]);
    });

    test('tolerates incomplete code while typing', () => {
        const { sliders } = extractControls(`$slider('foo', 0, 0, 5)\n$sine(`);
        expect(sliders.map((s) => s.label)).toEqual(['foo']);
    });
});

describe('resolveControls', () => {
    test('a control mid-edit keeps its last valid state, marked incomplete', () => {
        const valid = resolveControls(
            extractControls(`$slider('cut', 1, 0, 5); $toggleBtn('t', true);`),
            undefined,
        );
        expect(valid.sliders[0].incomplete).toBe(false);

        // The user has deleted the min value and not yet typed a new one.
        const editing = resolveControls(
            extractControls(`$slider('cut', 1, , 5); $toggleBtn('t', );`),
            valid,
        );
        // The last valid values hold; the call site is the current one.
        expect(editing.sliders).toEqual([
            { ...valid.sliders[0], incomplete: true },
        ]);
        expect(editing.buttons).toEqual([
            {
                ...valid.buttons[0],
                argsStart: valid.buttons[0].argsStart - 1,
                callStart: valid.buttons[0].callStart - 1,
                incomplete: true,
            },
        ]);

        // Still mid-edit on the next keystroke: the last valid state holds.
        const stillEditing = resolveControls(
            extractControls(`$slider('cut', 1, -, 5)`),
            editing,
        );
        expect(stillEditing.sliders[0]).toMatchObject({
            incomplete: true,
            min: 0,
        });

        const done = resolveControls(
            extractControls(`$slider('cut', 1, -1, 5)`),
            stillEditing,
        );
        expect(done.sliders[0]).toMatchObject({ incomplete: false, min: -1 });
    });

    test('a new call with no valid state yet is not shown', () => {
        const resolved = resolveControls(
            extractControls(`$slider('foo', 0, `),
            undefined,
        );
        expect(resolved.sliders).toEqual([]);
    });

    test('keeps the source order with incomplete controls in their slot', () => {
        const valid = resolveControls(
            extractControls(`$slider('a', 1, 0, 2); $slider('b', 1, 0, 2);`),
            undefined,
        );
        const editing = resolveControls(
            extractControls(`$slider('a', 1, , 2); $slider('b', 1, 0, 2);`),
            valid,
        );
        expect(editing.sliders.map((s) => [s.label, s.incomplete])).toEqual([
            ['a', true],
            ['b', false],
        ]);
    });
});

describe('extractControls groups', () => {
    test('places controls in their groups, nested, in source order', () => {
        const { sliders, buttons, groups } = extractControls(`
            const synth = $cGroup('Synth');
            const amp = synth.cGroup('Amp', true);
            $slider('top', 1, 0, 2);
            synth.slider('root', 'c3', 'c2', 'c5');
            amp.slider('level', 0.5, 0, 1);
            $cGroup('Fx').btn('kick');
        `);
        expect(groups.map((g) => [g.group, g.label, g.collapsed])).toEqual([
            [[], 'Synth', false],
            [['Synth'], 'Amp', true],
            [[], 'Fx', false],
        ]);
        expect(sliders.map((s) => [s.group, s.label])).toEqual([
            [[], 'top'],
            [['Synth'], 'root'],
            [['Synth', 'Amp'], 'level'],
        ]);
        expect(buttons.map((b) => [b.group, b.label])).toEqual([
            [['Fx'], 'kick'],
        ]);
    });

    test('labels repeat freely across groups but not within one', () => {
        const { sliders } = extractControls(`
            const a = $cGroup('A');
            const b = $cGroup('B');
            a.slider('cut', 1, 0, 2);
            b.slider('cut', 1, 0, 2);
            a.slider('cut', 2, 0, 2);
        `);
        expect(sliders.map((s) => s.group)).toEqual([['A'], ['B']]);
    });

    test('a call site records the method name, for anchors and jumps', () => {
        const source = `const g = $cGroup('G');\ng.slider('x', 1, 0, 2);`;
        const { sliders, groups } = extractControls(source);
        expect(sliders[0].callStart).toBe(source.indexOf('g.slider') + 2);
        expect(groups[0].callStart).toBe(source.indexOf('$cGroup'));
        expect(groups[0].argsStart).toBe(source.indexOf("'G'"));
    });

    test('a group call with invalid arguments shows but cannot collapse', () => {
        const { groups } = extractControls(`$cGroup('G', 1);`);
        expect(groups[0]).toMatchObject({
            collapseEdit: null,
            collapsed: false,
            label: 'G',
        });
    });

    test('omits controls whose group cannot be resolved', () => {
        const { sliders } = extractControls(
            `function voice(g) { g.slider('x', 1, 0, 2); }`,
        );
        expect(sliders).toEqual([]);
    });
});
