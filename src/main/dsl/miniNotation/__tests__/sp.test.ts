import { describe, expect, test } from 'vitest';

import { $sp, MiniParseError, isParsedPattern } from '../index';
import type { AtomValue, MiniAST } from '../ast';

function collectNumbers(ast: MiniAST, out: number[] = []): number[] {
    if ('Pure' in ast) {
        const atom = ast.Pure.node as AtomValue;
        if ('Number' in atom) out.push(atom.Number);
        return out;
    }
    if ('Rest' in ast) return out;
    if ('List' in ast) {
        for (const c of ast.List.node) collectNumbers(c, out);
        return out;
    }
    if ('Sequence' in ast) {
        for (const [c] of ast.Sequence) collectNumbers(c, out);
        return out;
    }
    if ('FastCat' in ast) {
        for (const [c] of ast.FastCat) collectNumbers(c, out);
        return out;
    }
    if ('SlowCat' in ast) {
        for (const [c] of ast.SlowCat) collectNumbers(c, out);
        return out;
    }
    if ('Stack' in ast) {
        for (const c of ast.Stack) collectNumbers(c, out);
        return out;
    }
    if ('RandomChoice' in ast) {
        for (const c of ast.RandomChoice[0]) collectNumbers(c, out);
        return out;
    }
    if ('Fast' in ast) return collectNumbers(ast.Fast[0], out);
    if ('Slow' in ast) return collectNumbers(ast.Slow[0], out);
    if ('Replicate' in ast) return collectNumbers(ast.Replicate[0], out);
    if ('Degrade' in ast) return collectNumbers(ast.Degrade[0], out);
    if ('Euclidean' in ast) return collectNumbers(ast.Euclidean.pattern, out);
    if ('Polymeter' in ast) {
        for (const c of ast.Polymeter.children) collectNumbers(c, out);
        return out;
    }
    return out;
}

function approxEq(a: number, b: number, eps = 1e-9): boolean {
    return Math.abs(a - b) < eps;
}

describe('$sp', () => {
    test('returns a ParsedPattern wrapper', () => {
        const r = $sp('0', 'c(major)');
        expect(isParsedPattern(r)).toBe(true);
        expect(r.source).toBe('0');
    });

    test('C major degrees resolve to 1V/oct voltages', () => {
        const r = $sp('0 2 4', 'c(major)');
        const v = collectNumbers(r.ast);
        // C4 = 0V, E4 (degree 2 = +4 semitones) = 4/12V,
        // G4 (degree 4 = +7 semitones) = 7/12V
        expect(approxEq(v[0], 0)).toBe(true);
        expect(approxEq(v[1], 4 / 12)).toBe(true);
        expect(approxEq(v[2], 7 / 12)).toBe(true);
    });

    test('octave in scale shifts the root', () => {
        const r = $sp('0', 'c3(major)');
        const v = collectNumbers(r.ast);
        // C3 = MIDI 48, root_v = (48 - 60)/12 = -1
        expect(approxEq(v[0], -1)).toBe(true);
    });

    test('negative degree wraps below the root', () => {
        const r = $sp('-1', 'c(major)');
        const v = collectNumbers(r.ast);
        // Degree -1 in C major is B3 (one semitone below C4) = -1/12
        expect(approxEq(v[0], -1 / 12)).toBe(true);
    });

    test('octave-crossing degree', () => {
        const r = $sp('7 8', 'c(major)');
        const v = collectNumbers(r.ast);
        // Degree 7 = C5 = 1V, degree 8 = D5 = 1 + 2/12
        expect(approxEq(v[0], 1)).toBe(true);
        expect(approxEq(v[1], 1 + 2 / 12)).toBe(true);
    });

    test('custom intervals (pentatonic)', () => {
        const r = $sp('0 1 2 3 4', 'c(0 2 4 7 9)');
        const v = collectNumbers(r.ast);
        // Steps: 0, 2, 4, 7, 9 semitones from root C4
        expect(approxEq(v[0], 0)).toBe(true);
        expect(approxEq(v[1], 2 / 12)).toBe(true);
        expect(approxEq(v[2], 4 / 12)).toBe(true);
        expect(approxEq(v[3], 7 / 12)).toBe(true);
        expect(approxEq(v[4], 9 / 12)).toBe(true);
    });

    test('just intonation differs from 12-TET chromatic', () => {
        // Both scales are 12 chromatic steps; only the tuning table
        // differs. Degree 4 in just intonation is a pure major third
        // (5/4 ratio = log2(1.25) V/oct), versus 4/12 V/oct in 12-TET.
        const just = $sp('4', 'c(just)');
        const chromatic = $sp('4', 'c(chromatic)');
        const vJust = collectNumbers(just.ast)[0];
        const vChrom = collectNumbers(chromatic.ast)[0];
        expect(approxEq(vJust, Math.log2(1.25))).toBe(true);
        expect(approxEq(vChrom, 4 / 12)).toBe(true);
        expect(vJust).not.toBeCloseTo(vChrom, 6);
    });

    test('rejects non-integer atom', () => {
        expect(() => $sp('1.5', 'c(major)')).toThrow(MiniParseError);
    });

    test('rejects note atom', () => {
        expect(() => $sp('c4', 'c(major)')).toThrow(MiniParseError);
    });

    test('rejects Hz atom', () => {
        expect(() => $sp('440hz', 'c(major)')).toThrow(MiniParseError);
    });

    test('rejects bad scale string', () => {
        expect(() => $sp('0', 'nonsense')).toThrow();
    });

    test('rest passes through', () => {
        const r = $sp('0 ~ 4', 'c(major)');
        expect('Sequence' in r.ast).toBe(true);
        const seq = (r.ast as { Sequence: Array<[MiniAST, number | null]> })
            .Sequence;
        expect(seq.length).toBe(3);
        expect('Pure' in seq[0][0]).toBe(true);
        expect('Rest' in seq[1][0]).toBe(true);
        expect('Pure' in seq[2][0]).toBe(true);
    });

    test('euclidean modifier preserved over voltage atoms', () => {
        const r = $sp('0(3,8)', 'c(major)');
        expect('Euclidean' in r.ast).toBe(true);
        const inner = (r.ast as { Euclidean: { pattern: MiniAST } }).Euclidean
            .pattern;
        // Pattern atom should be the resolved voltage for degree 0 (= 0V).
        const v = collectNumbers(inner);
        expect(approxEq(v[0], 0)).toBe(true);
    });

    test('source string preserved', () => {
        const r = $sp('0 2 4', 'c(major)');
        expect(r.source).toBe('0 2 4');
    });

    test('non-string source throws', () => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        expect(() => $sp(123 as any, 'c(major)')).toThrow(MiniParseError);
    });

    test('non-string scale throws', () => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        expect(() => $sp('0', 123 as any)).toThrow(MiniParseError);
    });
});

describe('$sp rest atoms', () => {
    test('`~` rest passes through', () => {
        const r = $sp('0 ~ 2', 'c(major)');
        if ('Sequence' in r.ast) {
            expect('Rest' in r.ast.Sequence[1][0]).toBe(true);
        }
    });

    test('`-` rest alternative passes through', () => {
        const r = $sp('0 - 2', 'c(major)');
        if ('Sequence' in r.ast) {
            expect('Rest' in r.ast.Sequence[1][0]).toBe(true);
        }
    });
});

describe('$sp mini-notation grammar', () => {
    test('fast subsequence `[a b c]` preserved', () => {
        const r = $sp('[0 2 4]', 'c(major)');
        expect('FastCat' in r.ast).toBe(true);
        const nums = collectNumbers(r.ast);
        expect(nums.length).toBe(3);
    });

    test('slow subsequence `<a b c>` preserved', () => {
        const r = $sp('<0 2 4>', 'c(major)');
        expect('SlowCat' in r.ast).toBe(true);
        expect(collectNumbers(r.ast).length).toBe(3);
    });

    test('stack `a, b` preserved', () => {
        const r = $sp('0 2, 4 7', 'c(major)');
        expect('Stack' in r.ast).toBe(true);
        expect(collectNumbers(r.ast).length).toBe(4);
    });

    test('random choice `a|b|c` preserved', () => {
        const r = $sp('0|2|4', 'c(major)');
        // Top-level is sequence containing one RandomChoice element.
        const nums = collectNumbers(r.ast);
        expect(nums.length).toBe(3);
    });

    test('polymeter `{...}` preserved with voltage atoms', () => {
        const r = $sp('{0 2, 4 5 7}', 'c(major)');
        expect('Polymeter' in r.ast).toBe(true);
        const nums = collectNumbers(r.ast);
        expect(nums.length).toBe(5);
    });

    test('polymeter with explicit step count', () => {
        const r = $sp('{0 2 4}%4', 'c(major)');
        expect('Polymeter' in r.ast).toBe(true);
        if ('Polymeter' in r.ast) {
            expect(r.ast.Polymeter.steps_per_cycle).not.toBe(null);
        }
    });

    test('feet `.` inside polymeter parses', () => {
        const r = $sp('{0 . 2 4, 5 7 . 9}', 'c(major)');
        expect('Polymeter' in r.ast).toBe(true);
    });
});

describe('$sp modifiers', () => {
    test('weight `@n` preserved', () => {
        const r = $sp('0@2 2', 'c(major)');
        if ('Sequence' in r.ast) {
            expect(r.ast.Sequence[0][1]).toBe(2);
        }
    });

    test('elongation `_` extends preceding weight', () => {
        const r = $sp('0 _ _', 'c(major)');
        if ('Sequence' in r.ast) {
            expect(r.ast.Sequence.length).toBe(1);
            expect(r.ast.Sequence[0][1]).toBe(3);
        }
    });

    test('speed up `*n` wraps voltage atoms', () => {
        const r = $sp('0*3', 'c(major)');
        expect('Fast' in r.ast).toBe(true);
    });

    test('slow down `/n` wraps voltage atoms', () => {
        const r = $sp('0/2', 'c(major)');
        expect('Slow' in r.ast).toBe(true);
    });

    test('replicate `!n` produces n copies', () => {
        const r = $sp('0!3', 'c(major)');
        expect('Replicate' in r.ast).toBe(true);
    });

    test('degrade `?` and `?n` preserved', () => {
        const r1 = $sp('0?', 'c(major)');
        const r2 = $sp('0?0.8', 'c(major)');
        expect('Degrade' in r1.ast).toBe(true);
        expect('Degrade' in r2.ast).toBe(true);
    });

    test('euclidean `(k,n,offset)` with offset preserved', () => {
        const r = $sp('0(3,8,1)', 'c(major)');
        expect('Euclidean' in r.ast).toBe(true);
        if ('Euclidean' in r.ast) {
            expect(r.ast.Euclidean.rotation).not.toBe(null);
        }
    });
});

describe('$sp scale formats', () => {
    test('named minor scale', () => {
        const r = $sp('0 1 2 3', 'a(min)');
        // A minor intervals from A: [0,2,3,5,7,8,10]
        // Root A = MIDI 69 (A4), root_v = (69-60)/12 = 9/12 = 0.75
        // Degree N → root_v + intervals[N]/12
        const v = collectNumbers(r.ast);
        expect(approxEq(v[0], 9 / 12)).toBe(true);
        expect(approxEq(v[1], (9 + 2) / 12)).toBe(true);
        expect(approxEq(v[2], (9 + 3) / 12)).toBe(true);
        expect(approxEq(v[3], (9 + 5) / 12)).toBe(true);
    });

    test('dorian mode', () => {
        const r = $sp('0 1 2', 'd(dorian)');
        // D dorian from D4: D, E, F = MIDI 62, 64, 65
        // root_v = (62-60)/12 = 2/12
        const v = collectNumbers(r.ast);
        expect(approxEq(v[0], 2 / 12)).toBe(true);
        expect(approxEq(v[1], 4 / 12)).toBe(true);
        expect(approxEq(v[2], 5 / 12)).toBe(true);
    });

    test('chromatic scale resolves chromatically from C4', () => {
        const r = $sp('0 1 2 12', 'chromatic');
        const v = collectNumbers(r.ast);
        // chromatic has no tonic — N-API helper falls back to MIDI 60 root.
        expect(approxEq(v[0], 0)).toBe(true);
        expect(approxEq(v[1], 1 / 12)).toBe(true);
        expect(approxEq(v[2], 2 / 12)).toBe(true);
        expect(approxEq(v[3], 1)).toBe(true);
    });

    test('pythagorean tuning', () => {
        // Pythagorean fifth (degree 7 of c(pythagorean)) is exact 3/2.
        const r = $sp('7', 'c(pythagorean)');
        const v = collectNumbers(r.ast)[0];
        expect(approxEq(v, Math.log2(1.5))).toBe(true);
    });

    test('custom intervals with explicit octave', () => {
        const r = $sp('0', 'a3(0 2 4 5 7 9 11)');
        // A3 = MIDI 57, root_v = (57-60)/12 = -3/12
        const v = collectNumbers(r.ast)[0];
        expect(approxEq(v, -3 / 12)).toBe(true);
    });

    test('flat accidental in root', () => {
        const r = $sp('0', 'eb(major)');
        // Eb4 = MIDI 63, root_v = 3/12
        const v = collectNumbers(r.ast)[0];
        expect(approxEq(v, 3 / 12)).toBe(true);
    });
});

describe('$sp edge cases', () => {
    test('empty source throws', () => {
        expect(() => $sp('', 'c(major)')).toThrow(MiniParseError);
    });

    test('unclosed bracket throws', () => {
        expect(() => $sp('[0 2', 'c(major)')).toThrow(MiniParseError);
    });

    test('zero degrees in pattern (only rests) calls N-API with empty array', () => {
        const r = $sp('~ ~', 'c(major)');
        // Should still produce a valid pattern with no numeric atoms.
        const nums = collectNumbers(r.ast);
        expect(nums.length).toBe(0);
    });

    test('combined: fast, weight, euclidean over voltage atoms', () => {
        const r = $sp('[0@2 2 4](3,8)', 'c(major)');
        expect('Euclidean' in r.ast).toBe(true);
    });

    test('all_spans matches source positions', () => {
        const r = $sp('0 2 4', 'c(major)');
        expect(r.all_spans.length).toBe(3);
        expect(r.all_spans).toContainEqual([0, 1]);
        expect(r.all_spans).toContainEqual([2, 3]);
        expect(r.all_spans).toContainEqual([4, 5]);
    });
});
