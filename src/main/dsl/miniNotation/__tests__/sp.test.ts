import { describe, expect, test } from 'vitest';

import {
    $sp,
    MiniParseError,
    isSpPattern,
    type SpAlignmentMode,
    type SpPattern,
} from '../index';
import type { MiniAST } from '../ast';
import { replaceSignals } from '../../GraphBuilder';

const MODES: SpAlignmentMode[] = [
    'in',
    'out',
    'mix',
    'squeeze',
    'squeezeout',
    'reset',
    'restart',
];

describe('$sp builder', () => {
    test('returns an SpPattern wrapper with the wire-shape discriminator', () => {
        const r = $sp('0 2 4', 'c(major)');
        expect(isSpPattern(r)).toBe(true);
        expect(r.__kind).toBe('SpPattern');
        expect(r.scale).toBe('c(major)');
        expect(r.ops).toEqual([]);
        expect(r.sources.length).toBe(1);
        expect(r.sources[0].source).toBe('0 2 4');
    });

    test('non-string source throws', () => {
        expect(() => $sp(123 as never, 'c(major)')).toThrow(MiniParseError);
    });

    test('non-string scale throws', () => {
        expect(() => $sp('0', 123 as never)).toThrow(MiniParseError);
    });

    test('argument_spans length tracks sources length', () => {
        const r = $sp('0', 'c(major)');
        expect(r.argument_spans.length).toBe(r.sources.length);
    });
});

describe('$sp chain methods', () => {
    test('.add(rhs) appends source + op with default mode "in"', () => {
        const r = $sp('0 2 4', 'c(maj)').add('0 2');
        expect(r.sources.length).toBe(2);
        expect(r.ops.length).toBe(1);
        expect(r.ops[0]).toEqual({ op: 'add', mode: 'in' });
        expect(r.sources[1].source).toBe('0 2');
        expect(r.argument_spans.length).toBe(2);
    });

    test('.sub(rhs) appends a sub op with default mode "in"', () => {
        const r = $sp('0 2 4', 'c(maj)').sub('1');
        expect(r.ops[0]).toEqual({ op: 'sub', mode: 'in' });
    });

    test('bare .add(x) is deep-equal to .add.in(x)', () => {
        const a = $sp('0 2 4', 'c(maj)').add('0 2');
        const b = $sp('0 2 4', 'c(maj)').add.in('0 2');
        expect(a.ops).toEqual(b.ops);
        expect(a.sources[1].source).toBe(b.sources[1].source);
    });

    test('all 7 modes are callable on .add and .sub', () => {
        const base = $sp('0 1 2', 'c(maj)');
        for (const mode of MODES) {
            const addR = (
                base.add as unknown as Record<string, (rhs: string) => SpPattern>
            )[mode]('0');
            expect(addR.ops[0]).toEqual({ op: 'add', mode });
            const subR = (
                base.sub as unknown as Record<string, (rhs: string) => SpPattern>
            )[mode]('0');
            expect(subR.ops[0]).toEqual({ op: 'sub', mode });
        }
    });

    test('chain accumulates: .add(...).sub.squeeze(...)', () => {
        const r = $sp('0 2 4', 'c(maj)').add('0 2').sub.squeeze('1');
        expect(r.sources.length).toBe(3);
        expect(r.ops.length).toBe(2);
        expect(r.ops[0]).toEqual({ op: 'add', mode: 'in' });
        expect(r.ops[1]).toEqual({ op: 'sub', mode: 'squeeze' });
    });

    test('chain is immutable — original SpPattern unchanged', () => {
        const base = $sp('0', 'c(maj)');
        const chained = base.add('1');
        expect(base.sources.length).toBe(1);
        expect(base.ops.length).toBe(0);
        expect(chained.sources.length).toBe(2);
    });

    test('non-string RHS throws MiniParseError', () => {
        const base = $sp('0', 'c(maj)');
        expect(() => base.add(123 as never)).toThrow(MiniParseError);
        expect(() => base.sub.squeeze(null as never)).toThrow(MiniParseError);
    });
});

describe('$sp JSON serialization', () => {
    test('JSON.stringify produces the wire-shape (no helper methods)', () => {
        const r = $sp('0 2', 'c(maj)').add('1');
        const json = JSON.parse(JSON.stringify(r));
        expect(json.__kind).toBe('SpPattern');
        expect(json.scale).toBe('c(maj)');
        expect(json.ops).toEqual([{ op: 'add', mode: 'in' }]);
        expect(json.sources.length).toBe(2);
        expect(json.argument_spans.length).toBe(2);
        // add / sub are non-enumerable property descriptors — must NOT
        // appear in the JSON.
        expect('add' in json).toBe(false);
        expect('sub' in json).toBe(false);
    });

    test('each source carries the standard ParsedPatternPayload fields', () => {
        const r = $sp('0 2 4', 'c(maj)').add('0 2');
        const json = JSON.parse(JSON.stringify(r));
        for (const s of json.sources) {
            expect(typeof s.source).toBe('string');
            expect(Array.isArray(s.all_spans)).toBe(true);
            // AST is the parsed MiniAST object.
            expect(typeof s.ast).toBe('object');
        }
    });
});

describe('$sp source AST integrity', () => {
    test('rest atoms pass through into the source AST', () => {
        const r = $sp('0 ~ 4', 'c(maj)');
        const ast = r.sources[0].ast as MiniAST;
        if ('Sequence' in ast) {
            expect('Rest' in ast.Sequence[1][0]).toBe(true);
        } else {
            throw new Error('Expected a sequence AST');
        }
    });

    test('chain RHS is parsed into its own AST', () => {
        const r = $sp('0 2 4', 'c(maj)').add('1 3');
        expect(r.sources[1].source).toBe('1 3');
        const ast = r.sources[1].ast as MiniAST;
        expect('Sequence' in ast).toBe(true);
    });

    test('euclidean modifier in source preserved', () => {
        const r = $sp('0(3,8)', 'c(maj)');
        const ast = r.sources[0].ast as MiniAST;
        expect('Euclidean' in ast).toBe(true);
    });
});

describe('$sp source string + spans', () => {
    test('source string is preserved verbatim', () => {
        const r = $sp('0 2 4', 'c(maj)');
        expect(r.sources[0].source).toBe('0 2 4');
    });

    test('all_spans count matches the number of atoms', () => {
        const r = $sp('0 2 4', 'c(maj)');
        expect(r.sources[0].all_spans.length).toBe(3);
    });
});

describe('$sp opaque payload preservation through replaceSignals', () => {
    test('null weights in Sequence AST survive replaceSignals (no null→0 collapse)', () => {
        // Regression: SpPattern was not opaque-guarded in replaceValues, so
        // walking its sources[].ast tree collapsed Sequence weights from
        // null → 0 via valueToSignal, producing zero-duration haps and
        // silent audio. Sources with a single atom (no Sequence wrapping)
        // were unaffected, masking the bug as "works for '0', fails for '0 1'".
        const pat = $sp('0 1', 'c(maj)');
        const walked = replaceSignals(pat) as SpPattern;
        const seq = (walked.sources[0].ast as { Sequence?: Array<[unknown, unknown]> }).Sequence;
        expect(seq).toBeDefined();
        expect(seq).toHaveLength(2);
        expect(seq![0][1]).toBeNull();
        expect(seq![1][1]).toBeNull();
    });

    test('chain ops survive replaceSignals', () => {
        const pat = $sp('0 1', 'c(maj)').add('1');
        const walked = replaceSignals(pat) as SpPattern;
        expect(walked.ops).toEqual([{ op: 'add', mode: 'in' }]);
        expect(walked.sources).toHaveLength(2);
    });
});
