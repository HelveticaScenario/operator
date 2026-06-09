import { describe, expect, test } from 'vitest';

import {
    $p,
    MiniParseError,
    isArrangePattern,
    type ArrangePattern,
} from '../index';
import { replaceSignals } from '../../GraphBuilder';

describe('$p.arrange builder', () => {
    test('builds an ArrangePattern wire object from [cycles, pattern] tuples', () => {
        const r = $p.arrange([2, $p('c4 e4')], [4, $p.s('0 2 4', 'c(major)')]);

        expect(isArrangePattern(r)).toBe(true);
        expect(r.__kind).toBe('ArrangePattern');
        expect(r.sections).toHaveLength(2);

        expect(r.sections[0].cycles).toBe(2);
        expect(r.sections[0].pattern.__kind).toBe('ParsedPattern');
        expect(r.sections[1].cycles).toBe(4);
        expect(r.sections[1].pattern.__kind).toBe('SpPattern');

        // Flat argument_spans: one per source across sections, in order.
        expect(r.argument_spans).toHaveLength(2);
    });

    test('encodes Infinity as the string sentinel "Infinity"', () => {
        const r = $p.arrange([2, $p('c4')], [Infinity, $p.s('0 2 4', 'a(min)')]);
        expect(r.sections[0].cycles).toBe(2);
        expect(r.sections[1].cycles).toBe('Infinity');
    });

    test('nests: an ArrangePattern is a valid section, spans flatten', () => {
        const nested = $p.arrange([1, $p('e4')], [1, $p('g4')]);
        expect(nested.argument_spans).toHaveLength(2);

        const r = $p.arrange([2, $p('c4')], [2, nested]);
        expect(r.sections[1].pattern.__kind).toBe('ArrangePattern');
        // 1 (c4) + 2 (nested's two sources) = 3 flat spans.
        expect(r.argument_spans).toHaveLength(3);
    });

    test('flattens chained $p.s argument spans across sections', () => {
        // A chained $p.s contributes one span per source in the chain.
        const chained = $p.s('0 2 4', 'c(maj)').add('0 5');
        expect(chained.argument_spans).toHaveLength(2);

        const r = $p.arrange([1, $p('c4')], [1, chained]);
        // 1 (c4) + 2 (chain) = 3.
        expect(r.argument_spans).toHaveLength(3);
    });

    test('rejects an empty arrangement', () => {
        expect(() => ($p.arrange as () => ArrangePattern)()).toThrow(
            MiniParseError,
        );
    });

    test('rejects non-integer, zero, and negative cycle counts', () => {
        for (const bad of [2.5, 0, -1]) {
            expect(() => $p.arrange([bad, $p('c4')])).toThrow(MiniParseError);
        }
    });

    test('rejects a non-[cycles, pattern] section', () => {
        expect(() =>
            $p.arrange([2] as unknown as [number, ReturnType<typeof $p>]),
        ).toThrow(MiniParseError);
    });

    test('rejects a non-pattern section payload', () => {
        expect(() =>
            $p.arrange([
                2,
                'c4' as unknown as ReturnType<typeof $p>,
            ]),
        ).toThrow(MiniParseError);
    });

    test('rejects an Infinity section that is not last', () => {
        expect(() =>
            $p.arrange([Infinity, $p('c4')], [2, $p('e4')]),
        ).toThrow(MiniParseError);
    });

    test('survives replaceSignals verbatim (no AST collapse)', () => {
        const r = $p.arrange([2, $p('c4')], [2, $p.s('0 2 4', 'c(maj)')]);
        const out = replaceSignals(r) as ArrangePattern;

        expect(out.__kind).toBe('ArrangePattern');
        expect(out.sections).toHaveLength(2);
        // Nested mini-notation payloads must be preserved, not deep-walked
        // (which would collapse null accidental/octave/weight slots to 0).
        const sp = out.sections[1].pattern as { sources: { source: string }[] };
        expect(sp.sources[0].source).toBe('0 2 4');
    });
});
