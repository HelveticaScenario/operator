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

    test('resolves inline chains and const-bound groups', () => {
        expect(
            calls(`
                const g = $cGroup('G');
                const h = (g).cGroup('H');
                h.toggleBtn('t', false);
                $cGroup('I').cGroup('J').btn('b');
            `),
        ).toEqual([
            ['$cGroup', 'G', []],
            ['$cGroup', 'H', ['G']],
            ['$toggleBtn', 't', ['G', 'H']],
            ['$cGroup', 'I', []],
            ['$cGroup', 'J', ['I']],
            ['$btn', 'b', ['I', 'J']],
        ]);
    });

    test('a function body can use a group const declared after it', () => {
        expect(
            calls(`
                function voice() { g.btn('b'); }
                const g = $cGroup('G');
                voice();
            `)[0],
        ).toEqual(['$btn', 'b', ['G']]);
    });

    test('method calls on a parameter, let, or computed receiver are not reported', () => {
        expect(
            calls(`
                function voice(g) { g.btn('a'); }
                let l = $cGroup('L');
                l.btn('b');
                const pick = true ? $cGroup('X') : $cGroup('Y');
                pick.btn('c');
                $cGroup(name).btn('d');
                someObject.slider('e');
            `).filter(([kind]) => kind === '$btn' || kind === '$slider'),
        ).toEqual([]);
    });

    test('$-prefixed names are not group methods', () => {
        expect(calls(`$cGroup('G').$slider('x', 1, 0, 2);`)).toEqual([
            ['$cGroup', 'G', []],
        ]);
    });

    test('an inner binding shadows an outer group', () => {
        expect(
            calls(`
                const g = $cGroup('G');
                function f() { const g = {}; g.btn('b'); }
            `),
        ).toEqual([['$cGroup', 'G', []]]);
    });
});
