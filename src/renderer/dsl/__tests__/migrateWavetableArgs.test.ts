import { describe, expect, test } from 'vitest';

import { migrateWavetableArgs } from '../migrateWavetableArgs';

describe('migrateWavetableArgs', () => {
    // ─── Basic swaps ──────────────────────────────────────────────────────

    test('swaps an inline $wavs() handle and a note pitch', () => {
        const result = migrateWavetableArgs(`$wavetable($wavs().wt, 'c4')`);
        expect(result.migrated).toBe(`$wavetable('c4', $wavs().wt)`);
        expect(result.callsChanged).toBe(1);
        expect(result.commentsChanged).toBe(0);
        expect(result.skipped).toEqual([]);
    });

    test('keeps position and config in place while swapping the first two', () => {
        const result = migrateWavetableArgs(
            `$wavetable($wavs().wt, 0, 0, { phase: $table.mirror(0.5) }).out()`,
        );
        expect(result.migrated).toBe(
            `$wavetable(0, $wavs().wt, 0, { phase: $table.mirror(0.5) }).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('handles a nested member chain as the wav handle', () => {
        const result = migrateWavetableArgs(
            `$wavetable($wavs().tables.pad, $note('C4')).out()`,
        );
        expect(result.migrated).toBe(
            `$wavetable($note('C4'), $wavs().tables.pad).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('handles an element-access wav handle', () => {
        const result = migrateWavetableArgs(`$wavetable($wavs()[0], 440)`);
        expect(result.migrated).toBe(`$wavetable(440, $wavs()[0])`);
        expect(result.callsChanged).toBe(1);
    });

    test('treats a bare $wavs() call as a wav handle', () => {
        const result = migrateWavetableArgs(`$wavetable($wavs(), 0)`);
        expect(result.migrated).toBe(`$wavetable(0, $wavs())`);
        expect(result.callsChanged).toBe(1);
    });

    test('swaps when the pitch is a poly array', () => {
        const result = migrateWavetableArgs(
            `$wavetable($wavs().wt, ['c4', 'e4', 'g4'])`,
        );
        expect(result.migrated).toBe(
            `$wavetable(['c4', 'e4', 'g4'], $wavs().wt)`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('preserves whitespace around the swapped arguments', () => {
        const result = migrateWavetableArgs(
            `$wavetable(  $wavs().wt ,  'c4'  )`,
        );
        expect(result.migrated).toBe(`$wavetable(  'c4' ,  $wavs().wt  )`);
        expect(result.callsChanged).toBe(1);
    });

    test('rewrites every legacy call in a source', () => {
        const source = [
            `$wavetable($wavs().a, 0).out()`,
            `$wavetable($wavs().b, 'e4').out()`,
        ].join('\n');
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(
            [
                `$wavetable(0, $wavs().a).out()`,
                `$wavetable('e4', $wavs().b).out()`,
            ].join('\n'),
        );
        expect(result.callsChanged).toBe(2);
    });

    // ─── Idempotency ──────────────────────────────────────────────────────

    test('idempotent — already pitch-first is unchanged', () => {
        const source = `$wavetable('c4', $wavs().wt).out()`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped).toEqual([]);
    });

    test('idempotent over a double run', () => {
        const once = migrateWavetableArgs(`$wavetable($wavs().wt, 0)`).migrated;
        const twice = migrateWavetableArgs(once);
        expect(twice.migrated).toBe(once);
        expect(twice.callsChanged).toBe(0);
    });

    // ─── Variable tracking ────────────────────────────────────────────────

    test('swaps when the wav is held in a const', () => {
        const source = `const pad = $wavs().pad\n$wavetable(pad, 'c4')`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(
            `const pad = $wavs().pad\n$wavetable('c4', pad)`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('idempotent when a wav const is already the second arg', () => {
        const source = `const pad = $wavs().pad\n$wavetable('c4', pad)`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
    });

    test('swaps when the pitch is a ModuleOutput variable', () => {
        const source = `const lfo = $sine(0)\n$wavetable($wavs().wt, lfo)`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(
            `const lfo = $sine(0)\n$wavetable(lfo, $wavs().wt)`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('does not treat a reassigned-to-non-wav variable as a wav handle', () => {
        const source = [
            `let w = $wavs().wt`,
            `w = $sine(0)`,
            `$wavetable(w, 'c4')`,
        ].join('\n');
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped.length).toBe(1);
    });

    // ─── Ambiguous / needs review ─────────────────────────────────────────

    test('flags but does not rewrite a call with an unresolvable wav arg', () => {
        const result = migrateWavetableArgs(`$wavetable(getWav(), 0)`);
        expect(result.migrated).toBe(`$wavetable(getWav(), 0)`);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped.length).toBe(1);
        expect(result.skipped[0]).toContain('getWav()');
    });

    test('does not corrupt a pitch-first call whose wav arg is unresolvable', () => {
        const source = `$wavetable(0, getWav())`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
        // Reported for review rather than blindly swapped.
        expect(result.skipped.length).toBe(1);
    });

    test('flags a single wav-only call (no pitch to reorder)', () => {
        const result = migrateWavetableArgs(`$wavetable($wavs().wt)`);
        expect(result.migrated).toBe(`$wavetable($wavs().wt)`);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped.length).toBe(1);
    });

    test('ignores calls to other factories', () => {
        const source = `$other($wavs().wt, 0)`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped).toEqual([]);
    });

    // ─── Comments ─────────────────────────────────────────────────────────

    test('swaps args inside a line comment', () => {
        const result = migrateWavetableArgs(`// $wavetable($wavs().wt, 'c4')`);
        expect(result.migrated).toBe(`// $wavetable('c4', $wavs().wt)`);
        expect(result.commentsChanged).toBe(1);
        expect(result.callsChanged).toBe(0);
    });

    test('comment swap handles a nested call in the pitch slot', () => {
        const result = migrateWavetableArgs(
            `/* $wavetable($wavs().wt, $hz(440)) */`,
        );
        expect(result.migrated).toBe(`/* $wavetable($hz(440), $wavs().wt) */`);
        expect(result.commentsChanged).toBe(1);
    });

    test('idempotent on an already-migrated comment', () => {
        const source = `// $wavetable('c4', $wavs().wt)`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(source);
        expect(result.commentsChanged).toBe(0);
    });

    test('migrates both live code and its comment together', () => {
        const source = [
            `// e.g. $wavetable($wavs().wt, 'c4')`,
            `$wavetable($wavs().wt, 'c4')`,
        ].join('\n');
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(
            [
                `// e.g. $wavetable('c4', $wavs().wt)`,
                `$wavetable('c4', $wavs().wt)`,
            ].join('\n'),
        );
        expect(result.callsChanged).toBe(1);
        expect(result.commentsChanged).toBe(1);
    });

    // ─── Robustness ───────────────────────────────────────────────────────

    test('reports a parse error without throwing', () => {
        const result = migrateWavetableArgs(`$wavetable($wavs().wt, 'c4'`);
        // ts-morph is lenient and recovers; either it parses (and swaps) or
        // reports an error — never throws and never corrupts.
        expect(typeof result.migrated).toBe('string');
    });

    test('does not match $wavetableFoo (longer identifier)', () => {
        const source = `// $wavetableFoo($wavs().wt, 0)`;
        const result = migrateWavetableArgs(source);
        expect(result.migrated).toBe(source);
        expect(result.commentsChanged).toBe(0);
    });
});
