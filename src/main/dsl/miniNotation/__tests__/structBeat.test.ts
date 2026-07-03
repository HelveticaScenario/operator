import { describe, expect, test } from 'vitest';

import {
    $p,
    MiniParseError,
    isBeatPattern,
    isFastPattern,
    isParsedPattern,
    isStructPattern,
} from '../index';
import { replaceSignals } from '../../GraphBuilder';

describe('.struct builder', () => {
    test('$p(...).struct(s) builds a StructPattern wrapping the inner pattern', () => {
        const r = $p('c4 e4').struct('x ~ x x');

        expect(isStructPattern(r)).toBe(true);
        expect(r.__kind).toBe('StructPattern');
        expect(r.pattern.__kind).toBe('ParsedPattern');
        expect(r.bool_pattern.source).toBe('x ~ x x');
        // "x ~ x x" is a four-step sequence, not a single Pure atom.
        expect('Sequence' in r.bool_pattern.ast).toBe(true);
    });

    test('accepts numeric bool atoms and full mini-notation', () => {
        expect($p('c4').struct('1 0 1 1').__kind).toBe('StructPattern');
        expect($p('c4').struct('x ~ <x ~>').__kind).toBe('StructPattern');
        expect($p('c4').struct('x(3,8)').__kind).toBe('StructPattern');
    });

    test('rejects a non-string bool pattern', () => {
        // @ts-expect-error deliberate misuse
        expect(() => $p('c4').struct(3)).toThrow(MiniParseError);
    });

    test('rejects an unparseable bool pattern', () => {
        // `x4` / `xx` are not atoms: `x` must stand alone.
        expect(() => $p('c4').struct('x4')).toThrow(MiniParseError);
        expect(() => $p('c4').struct('xx')).toThrow(MiniParseError);
    });

    test('the bool pattern always appends an argument span', () => {
        expect($p('c4 e4').struct('x ~').argument_spans).toHaveLength(2);
        const chained = $p.s('0 2 4', 'c(maj)').add('0 5').struct('x ~');
        expect(chained.argument_spans).toHaveLength(3);
    });

    test('methods are non-enumerable — only the wire shape serializes', () => {
        const r = $p('c4').struct('x ~');
        expect(Object.keys(r).sort()).toEqual([
            '__kind',
            'argument_spans',
            'bool_pattern',
            'pattern',
        ]);
        const json: { __kind: unknown; pattern: Record<string, unknown> } =
            JSON.parse(JSON.stringify(r));
        expect(json.__kind).toBe('StructPattern');
        expect('struct' in json).toBe(false);
        expect('beat' in json).toBe(false);
        expect('fast' in json.pattern).toBe(false);
    });

    test('survives replaceSignals verbatim (no AST collapse)', () => {
        const out = replaceSignals($p('c4 e4').struct('x ~ x x'));
        expect(isStructPattern(out)).toBe(true);
        if (!isStructPattern(out)) return;
        expect(isParsedPattern(out.pattern)).toBe(true);
        expect(out.bool_pattern.source).toBe('x ~ x x');
    });
});

describe('.beat builder', () => {
    test('$p(...).beat(t, div) builds a BeatPattern wrapping the inner pattern', () => {
        const r = $p('c2').beat('0,7,10', 16);

        expect(isBeatPattern(r)).toBe(true);
        expect(r.__kind).toBe('BeatPattern');
        expect(r.pattern.__kind).toBe('ParsedPattern');
        if (typeof r.t === 'number') {
            throw new Error('expected a pattern t, got a number');
        }
        expect(r.t.source).toBe('0,7,10');
        // "0,7,10" is a comma stack — one onset per index.
        expect('Stack' in r.t.ast).toBe(true);
        // A numeric div is a raw constant, not a parsed pattern.
        expect(r.div).toBe(16);
    });

    test('t and div each accept a number or a mini-notation string', () => {
        const both = $p('c2').beat(0, 4);
        expect(both.t).toBe(0);
        expect(both.div).toBe(4);

        const divPattern = $p('c2').beat('0,7', '<16 8>');
        if (typeof divPattern.div === 'number') {
            throw new Error('expected a pattern div, got a number');
        }
        expect(divPattern.div.source).toBe('<16 8>');
    });

    test('string args append argument spans in t-then-div order', () => {
        // Both numbers: only the wrapped pattern's span.
        expect($p('c2').beat(0, 16).argument_spans).toHaveLength(1);
        // String t only.
        expect($p('c2').beat('0,7', 16).argument_spans).toHaveLength(2);
        // String t and div.
        expect($p('c2').beat('0,7', '<16 8>').argument_spans).toHaveLength(3);
    });

    test('rejects non-finite number args', () => {
        expect(() => $p('c2').beat(NaN, 16)).toThrow(MiniParseError);
        expect(() => $p('c2').beat(0, Infinity)).toThrow(MiniParseError);
    });

    test('methods are non-enumerable — only the wire shape serializes', () => {
        const r = $p('c2').beat('0,7', 16);
        expect(Object.keys(r).sort()).toEqual([
            '__kind',
            'argument_spans',
            'div',
            'pattern',
            't',
        ]);
        const json: { __kind: unknown } = JSON.parse(JSON.stringify(r));
        expect(json.__kind).toBe('BeatPattern');
        expect('beat' in json).toBe(false);
    });

    test('survives replaceSignals verbatim (no AST collapse)', () => {
        const out = replaceSignals($p('c2 g2').beat('0,7,10', 16));
        expect(isBeatPattern(out)).toBe(true);
        if (!isBeatPattern(out)) return;
        expect(isParsedPattern(out.pattern)).toBe(true);
        if (typeof out.t === 'number') {
            throw new Error('expected a pattern t, got a number');
        }
        expect(out.t.source).toBe('0,7,10');
    });
});

describe('.struct / .beat composition', () => {
    test('chain and nest with .fast/.slow in any order', () => {
        const r = $p('c4 e4').fast(2).struct('x ~ x x').beat('0,2', 4);
        expect(r.__kind).toBe('BeatPattern');
        expect(isStructPattern(r.pattern)).toBe(true);
        if (isStructPattern(r.pattern)) {
            expect(isFastPattern(r.pattern.pattern)).toBe(true);
        }

        const r2 = $p('c4').struct('x ~').fast(2);
        expect(r2.__kind).toBe('FastPattern');
        expect(isStructPattern(r2.pattern)).toBe(true);
    });

    test('work on $p.s and $p.arrange patterns', () => {
        const onSp = $p.s('0 2 4', 'c(maj)').struct('x ~ x');
        expect(onSp.__kind).toBe('StructPattern');
        expect(onSp.pattern.__kind).toBe('SpPattern');

        const onArrange = $p.arrange([2, $p('c4')], [2, $p('e4')]).beat(0, 4);
        expect(onArrange.__kind).toBe('BeatPattern');
        expect(onArrange.pattern.__kind).toBe('ArrangePattern');
    });

    test('valid as $p.arrange sections, spans flatten', () => {
        const r = $p.arrange(
            [2, $p('c4').struct('x ~')],
            [2, $p('e4').beat('0,7', 16)],
        );
        expect(r.sections[0].pattern.__kind).toBe('StructPattern');
        expect(r.sections[1].pattern.__kind).toBe('BeatPattern');
        // (c4 + bool) + (e4 + t) = 4 flat spans.
        expect(r.argument_spans).toHaveLength(4);
    });
});
