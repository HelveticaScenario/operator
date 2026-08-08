import { describe, expect, test } from 'vitest';

import {
    $p,
    MiniParseError,
    isSpPattern,
    type SpAlignmentMode,
    type SpPattern,
} from '../index';
import type { ParsedPattern } from '../ast';
import { parseMini } from '../parser';
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

describe('$p.s builder', () => {
    test('returns an SpPattern wrapper with the wire-shape discriminator', () => {
        const r = $p.s('0 2 4', 'c(major)');
        expect(isSpPattern(r)).toBe(true);
        expect(r.__kind).toBe('SpPattern');
        expect(r.scale).toBe('c(major)');
        expect(r.ops).toEqual([]);
        expect(r.sources.length).toBe(1);
        expect(r.sources[0].source).toBe('0 2 4');
    });

    test('non-string source throws', () => {
        expect(() => $p.s(123 as never, 'c(major)')).toThrow(MiniParseError);
    });

    test('non-string scale throws', () => {
        expect(() => $p.s('0', 123 as never)).toThrow(MiniParseError);
    });

    test('argument_spans length tracks sources length', () => {
        const r = $p.s('0', 'c(major)');
        expect(r.argument_spans.length).toBe(r.sources.length);
    });
});

describe('$p.s empty/whitespace source rejection', () => {
    test('empty source throws MiniParseError', () => {
        expect(() => $p.s('', 'c(maj)')).toThrow(MiniParseError);
    });

    test('whitespace-only source throws MiniParseError', () => {
        expect(() => $p.s('   ', 'c(maj)')).toThrow(MiniParseError);
    });

    test('empty chained .add(rhs) throws MiniParseError', () => {
        expect(() => $p.s('0 2', 'c(maj)').add('')).toThrow(MiniParseError);
    });

    test('whitespace-only chained .add(rhs) throws MiniParseError', () => {
        expect(() => $p.s('0 2', 'c(maj)').add('   ')).toThrow(MiniParseError);
    });

    test('empty chained .sub(rhs) throws MiniParseError', () => {
        expect(() => $p.s('0 2', 'c(maj)').sub('')).toThrow(MiniParseError);
    });

    test('a rest "~" source does NOT throw — it is a real atom', () => {
        expect(() => $p.s('~', 'c(maj)')).not.toThrow();
    });
});

describe('$p.s chain methods', () => {
    test('.add(rhs) appends source + op with default mode "in"', () => {
        const r = $p.s('0 2 4', 'c(maj)').add('0 2');
        expect(r.sources.length).toBe(2);
        expect(r.ops.length).toBe(1);
        expect(r.ops[0]).toEqual({ op: 'add', mode: 'in' });
        expect(r.sources[1].source).toBe('0 2');
        expect(r.argument_spans.length).toBe(2);
    });

    test('.sub(rhs) appends a sub op with default mode "in"', () => {
        const r = $p.s('0 2 4', 'c(maj)').sub('1');
        expect(r.ops[0]).toEqual({ op: 'sub', mode: 'in' });
    });

    test('bare .add(x) is deep-equal to .add.in(x)', () => {
        const a = $p.s('0 2 4', 'c(maj)').add('0 2');
        const b = $p.s('0 2 4', 'c(maj)').add.in('0 2');
        expect(a.ops).toEqual(b.ops);
        expect(a.sources[1].source).toBe(b.sources[1].source);
    });

    test('all 7 modes are callable on .add and .sub', () => {
        const base = $p.s('0 1 2', 'c(maj)');
        for (const mode of MODES) {
            const addR = (
                base.add as unknown as Record<
                    string,
                    (rhs: string) => SpPattern
                >
            )[mode]('0');
            expect(addR.ops[0]).toEqual({ op: 'add', mode });
            const subR = (
                base.sub as unknown as Record<
                    string,
                    (rhs: string) => SpPattern
                >
            )[mode]('0');
            expect(subR.ops[0]).toEqual({ op: 'sub', mode });
        }
    });

    test('chain accumulates: .add(...).sub.squeeze(...)', () => {
        const r = $p.s('0 2 4', 'c(maj)').add('0 2').sub.squeeze('1');
        expect(r.sources.length).toBe(3);
        expect(r.ops.length).toBe(2);
        expect(r.ops[0]).toEqual({ op: 'add', mode: 'in' });
        expect(r.ops[1]).toEqual({ op: 'sub', mode: 'squeeze' });
    });

    test('chain is immutable — original SpPattern unchanged', () => {
        const base = $p.s('0', 'c(maj)');
        const chained = base.add('1');
        expect(base.sources.length).toBe(1);
        expect(base.ops.length).toBe(0);
        expect(chained.sources.length).toBe(2);
    });

    test('non-string RHS throws MiniParseError', () => {
        const base = $p.s('0', 'c(maj)');
        expect(() => base.add(123 as never)).toThrow(MiniParseError);
        expect(() => base.sub.squeeze(null as never)).toThrow(MiniParseError);
    });
});

describe('$p.s JSON serialization', () => {
    test('JSON.stringify produces the wire-shape (no helper methods)', () => {
        const r = $p.s('0 2', 'c(maj)').add('1');
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
        const r = $p.s('0 2 4', 'c(maj)').add('0 2');
        const json = JSON.parse(JSON.stringify(r));
        for (const s of json.sources) {
            expect(typeof s.source).toBe('string');
            expect(Array.isArray(s.all_spans)).toBe(true);
            // AST is the parsed MiniAST object.
            expect(typeof s.ast).toBe('object');
        }
    });
});

describe('$p.s source AST integrity', () => {
    test('rest atoms pass through into the source AST', () => {
        const r = $p.s('0 ~ 4', 'c(maj)');
        const ast = r.sources[0].ast;
        if ('Sequence' in ast) {
            expect('Rest' in ast.Sequence[1][0]).toBe(true);
        } else {
            throw new Error('Expected a sequence AST');
        }
    });

    test('chain RHS is parsed into its own AST', () => {
        const r = $p.s('0 2 4', 'c(maj)').add('1 3');
        expect(r.sources[1].source).toBe('1 3');
        const ast = r.sources[1].ast;
        expect('Sequence' in ast).toBe(true);
    });

    test('euclidean modifier in source preserved', () => {
        const r = $p.s('0(3,8)', 'c(maj)');
        const ast = r.sources[0].ast;
        expect('Euclidean' in ast).toBe(true);
    });
});

describe('$p.s source string + spans', () => {
    test('source string is preserved verbatim', () => {
        const r = $p.s('0 2 4', 'c(maj)');
        expect(r.sources[0].source).toBe('0 2 4');
    });

    test('all_spans count matches the number of atoms', () => {
        const r = $p.s('0 2 4', 'c(maj)');
        expect(r.sources[0].all_spans.length).toBe(3);
    });
});

describe('$p.s opaque payload preservation through replaceSignals', () => {
    test('null weights in Sequence AST survive replaceSignals (no null→0 collapse)', () => {
        // Regression: SpPattern was not opaque-guarded in replaceValues, so
        // walking its sources[].ast tree collapsed Sequence weights from
        // null → 0 via valueToSignal, producing zero-duration haps and
        // silent audio. Sources with a single atom (no Sequence wrapping)
        // were unaffected, masking the bug as "works for '0', fails for '0 1'".
        const pat = $p.s('0 1', 'c(maj)');
        const walked = replaceSignals(pat) as SpPattern;
        const seq = (
            walked.sources[0].ast as { Sequence?: Array<[unknown, unknown]> }
        ).Sequence;
        expect(seq).toBeDefined();
        expect(seq).toHaveLength(2);
        expect(seq![0][1]).toBeNull();
        expect(seq![1][1]).toBeNull();
    });

    test('chain ops survive replaceSignals', () => {
        const pat = $p.s('0 1', 'c(maj)').add('1');
        const walked = replaceSignals(pat) as SpPattern;
        expect(walked.ops).toEqual([{ op: 'add', mode: 'in' }]);
        expect(walked.sources).toHaveLength(2);
    });
});

// Every peggy grammar construct, run through both $p.s and $p wrappers,
// asserts the walked AST is structurally identical to the raw parseMini()
// output. Catches any future regression where a wrapper variant drops
// out of the replaceValues opaque-payload allow-list. Null-bearing slots
// (Sequence weights, Note.accidental/octave, Euclidean.rotation,
// Polymeter.steps_per_cycle, Degrade weight) are the failure surface.
const GRAMMAR_CASES: Array<{ label: string; source: string }> = [
    { label: 'pure number', source: '0' },
    { label: 'pure negative number', source: '-3' },
    { label: 'pure Hz', source: '440hz' },
    { label: 'note with octave', source: 'c4' },
    { label: 'note bare letter (octave null)', source: 'c' },
    { label: 'note sharp', source: 'c#4' },
    { label: 'note flat', source: 'eb3' },
    { label: 'note flat no octave', source: 'eb' },
    { label: 'note b-flat no octave', source: 'bb' },
    { label: 'note f-alias flat no octave', source: 'cf' },
    { label: 'note s-alias sharp', source: 'cs4' },
    { label: 'rest tilde', source: '~' },
    { label: 'rest dash', source: '-' },
    { label: 'sequence (null weights)', source: '0 1 2' },
    { label: 'fast subsequence []', source: '[0 1] 2' },
    { label: 'slow subsequence <>', source: '<0 1 2>' },
    { label: 'stack via comma', source: '0,1,2' },
    { label: 'nested stack inside []', source: '[0 1, 2 3]' },
    { label: 'fast modifier *n', source: '0*2' },
    { label: 'slow modifier /n', source: '0/2' },
    { label: 'replicate !n', source: '0!3' },
    { label: 'replicate ! default', source: '0!' },
    { label: 'degrade ? with prob', source: '0?0.5' },
    { label: 'degrade ? default prob (null)', source: '0?' },
    { label: 'euclidean (k,n) no rotation', source: '0(3,8)' },
    { label: 'euclidean with rotation', source: '0(3,8,1)' },
    { label: 'fast factor as subsequence', source: 'c*[1 2]' },
    { label: 'weight @n positional', source: '0@2 1' },
    { label: 'random choice |', source: '0|1|2' },
    { label: 'rest inside choice', source: '0|~|2' },
    { label: 'patterned weight @<...>', source: '0@<1 2> 1' },
    { label: 'patterned weight @[...]', source: '0@[2 1] 1' },
    { label: 'patterned weight @{...}', source: '0@{1 2} 1' },
    { label: 'patterned weight bracketed choice', source: '0@[1|2] 1' },
    { label: 'patterned replicate !<...>', source: '0!<2 3> 1' },
    { label: 'patterned replicate ![...]', source: '0![2 3] 1' },
    { label: 'operand polymeter for fast', source: '0*{1 2 3}' },
    { label: 'operand polymeter with % steps', source: '0*{1 2 3}%4' },
    { label: 'euclid arg polymeter', source: '0(3,{4 8})' },
];

describe('peggy grammar survives replaceSignals (regression)', () => {
    for (const { label, source } of GRAMMAR_CASES) {
        test(`$p.s("${source}"): ${label}`, () => {
            const raw = parseMini(source);
            const pat = $p.s(source, 'c(maj)');
            const walked = replaceSignals(pat) as SpPattern;
            expect(walked.__kind).toBe('SpPattern');
            expect(walked.sources).toHaveLength(1);
            // Deep equality vs raw peggy output proves every null slot,
            // span tuple, and modifier branch survived the walk verbatim.
            expect(walked.sources[0].ast).toEqual(raw);
        });

        test(`$p("${source}"): ${label}`, () => {
            const raw = parseMini(source);
            const pat = $p(source);
            const walked = replaceSignals(pat) as ParsedPattern;
            expect(walked.__kind).toBe('ParsedPattern');
            expect(walked.ast).toEqual(raw);
        });
    }

    test('$p.s chain RHS AST also survives', () => {
        // Each chained source goes through the same parsePayload pipeline,
        // so the opaque guard must cover them too. Verify with a construct
        // that has null slots in both LHS and RHS.
        const lhs = '0 1';
        const rhs = '[0 1, 2 3]';
        const pat = $p.s(lhs, 'c(maj)').add(rhs);
        const walked = replaceSignals(pat) as SpPattern;
        expect(walked.sources[0].ast).toEqual(parseMini(lhs));
        expect(walked.sources[1].ast).toEqual(parseMini(rhs));
    });
});
