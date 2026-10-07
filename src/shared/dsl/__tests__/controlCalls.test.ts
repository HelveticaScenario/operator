import { describe, expect, test } from 'vitest';
import { ts } from 'ts-morph';

import { findControlCalls } from '../controlCalls';

function calls(source: string) {
    const sourceFile = ts.createSourceFile(
        'patch.js',
        source,
        ts.ScriptTarget.Latest,
        false,
        ts.ScriptKind.JS,
    );
    return findControlCalls(sourceFile).map((c) => [
        c.kind,
        (c.call.arguments[0] as ts.StringLiteral | undefined)?.text,
        c.group,
    ]);
}

describe('findControlCalls', () => {
    test('free calls belong to the root group', () => {
        expect(
            calls(`$slider('a', 1, 0, 2); $btn('b'); $cGroup('G');`),
        ).toEqual([
            ['$slider', 'a', []],
            ['$btn', 'b', []],
            ['$cGroup', 'G', []],
        ]);
    });

    test('resolves inline and const-bound group arguments', () => {
        expect(
            calls(`
                const g = $cGroup('G');
                const h = $cGroup('H', { group: (g) });
                $toggleBtn('t', false, h);
                $btn('b', $cGroup('J', { group: $cGroup('I') }));
                $slider('s', 1, 0, 2, g);
            `),
        ).toEqual([
            ['$cGroup', 'G', []],
            ['$cGroup', 'H', ['G']],
            ['$toggleBtn', 't', ['G', 'H']],
            ['$btn', 'b', ['I', 'J']],
            ['$cGroup', 'J', ['I']],
            ['$cGroup', 'I', []],
            ['$slider', 's', ['G']],
        ]);
    });

    test('a shorthand group property resolves its binding', () => {
        expect(
            calls(`
                const group = $cGroup('G');
                const h = $cGroup('H', { collapsed: true, group });
                $btn('b', h);
            `),
        ).toEqual([
            ['$cGroup', 'G', []],
            ['$cGroup', 'H', ['G']],
            ['$btn', 'b', ['G', 'H']],
        ]);
    });

    test('a function body can use a group const declared after it', () => {
        expect(
            calls(`
                function voice() { $btn('b', g); }
                const g = $cGroup('G');
                voice();
            `)[0],
        ).toEqual(['$btn', 'b', ['G']]);
    });

    test('a parameter, let, or computed group argument is not reported', () => {
        expect(
            calls(`
                function voice(g) { $btn('a', g); }
                let l = $cGroup('L');
                $btn('b', l);
                const pick = true ? $cGroup('X') : $cGroup('Y');
                $btn('c', pick);
                $btn('d', $cGroup(name));
                $slider('e', 1, 0, 2, someObject.group);
                $cGroup('K', { group: l });
                $cGroup('M', { ...opts });
                $cGroup('N', opts);
            `).filter(
                ([kind, label]) =>
                    kind !== '$cGroup' || ['K', 'M', 'N'].includes(String(label)),
            ),
        ).toEqual([]);
    });

    test('group methods are not control calls', () => {
        expect(calls(`$cGroup('G').slider('x', 1, 0, 2);`)).toEqual([
            ['$cGroup', 'G', []],
        ]);
    });

    test('an inner binding shadows an outer group', () => {
        expect(
            calls(`
                const g = $cGroup('G');
                function f() { const g = {}; $btn('b', g); }
            `),
        ).toEqual([['$cGroup', 'G', []]]);
    });

    test('params with a computed key, method, or accessor are not resolved', () => {
        expect(
            calls(`
                const g = $cGroup('G');
                $cGroup('A', { ['group']: g });
                $cGroup('B', { get group() { return g; } });
                $cGroup('C', { group() {} });
            `).map(([, label]) => label),
        ).toEqual(['G']);
    });

    test('loop, catch, and switch bindings shadow an outer group', () => {
        expect(
            calls(`
                const g = $cGroup('G');
                for (const g of [1]) { $btn('a', g); }
                for (let g = 0; g < 1; g++) { $btn('b', g); }
                for (const g in {}) { $btn('c', g); }
                try {} catch (g) { $btn('d', g); }
                switch (1) { case 1: const g = 2; $btn('e', g); }
                $btn('f', g);
            `).filter(([kind]) => kind === '$btn'),
        ).toEqual([['$btn', 'f', ['G']]]);
    });
});
