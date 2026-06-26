import { describe, expect, test } from 'vitest';

import { migrateChebyBlockDC } from '../migrateChebyBlockDC';

describe('migrateChebyBlockDC', () => {
    // ─── Direct calls ─────────────────────────────────────────────────────

    test('appends config to a bare $cheby(input, amount) call', () => {
        const result = migrateChebyBlockDC(`$cheby($sine('c4'), 3).out()`);
        expect(result.migrated).toBe(
            `$cheby($sine('c4'), 3, { blockDC: false }).out()`,
        );
        expect(result.callsChanged).toBe(1);
        expect(result.skipped).toEqual([]);
    });

    test('injects blockDC into an existing config object', () => {
        const result = migrateChebyBlockDC(
            `$cheby($sine('c4'), 3, { freq: 'c4' }).out()`,
        );
        expect(result.migrated).toBe(
            `$cheby($sine('c4'), 3, { blockDC: false, freq: 'c4' }).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('fills an empty config object cleanly', () => {
        const result = migrateChebyBlockDC(`$cheby(x, 3, {}).out()`);
        expect(result.migrated).toBe(`$cheby(x, 3, { blockDC: false }).out()`);
        expect(result.callsChanged).toBe(1);
    });

    // ─── Dollar-chain form ────────────────────────────────────────────────

    test('appends config to a .$.cheby(amount) chain call', () => {
        const result = migrateChebyBlockDC(`$sine('c4').$.cheby(3).out()`);
        expect(result.migrated).toBe(
            `$sine('c4').$.cheby(3, { blockDC: false }).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('injects blockDC into a .$.cheby config object', () => {
        const result = migrateChebyBlockDC(
            `$sine('c4').$.cheby(3, { freq: 'c4' })`,
        );
        expect(result.migrated).toBe(
            `$sine('c4').$.cheby(3, { blockDC: false, freq: 'c4' })`,
        );
        expect(result.callsChanged).toBe(1);
    });

    // ─── Mix-chain form (.$m injects a leading mix arg) ───────────────────

    test('appends config past the mix arg in a .$m.cheby(mix, amount) call', () => {
        const result = migrateChebyBlockDC(`$sine('c4').$m.cheby(2.5, 3).out()`);
        expect(result.migrated).toBe(
            `$sine('c4').$m.cheby(2.5, 3, { blockDC: false }).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('injects blockDC into a .$m.cheby config object', () => {
        const result = migrateChebyBlockDC(
            `$sine('c4').$m.cheby(2.5, 3, { freq: 'c4' })`,
        );
        expect(result.migrated).toBe(
            `$sine('c4').$m.cheby(2.5, 3, { blockDC: false, freq: 'c4' })`,
        );
        expect(result.callsChanged).toBe(1);
    });

    // ─── Idempotency ──────────────────────────────────────────────────────

    test('leaves a call that already sets blockDC untouched', () => {
        const src = `$cheby(x, 3, { blockDC: false }).out()`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });

    test('is idempotent across a second run', () => {
        const once = migrateChebyBlockDC(`$cheby(x, 3).out()`).migrated;
        const twice = migrateChebyBlockDC(once).migrated;
        expect(twice).toBe(once);
    });

    test('respects blockDC: true already set explicitly', () => {
        const src = `$cheby(x, 3, { blockDC: true })`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });

    test('detects blockDC set via a shorthand property', () => {
        const src = `$cheby(x, 3, { blockDC })`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });

    test('detects blockDC set via a string-literal key', () => {
        const src = `$cheby(x, 3, { 'blockDC': false })`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });

    test('detects blockDC set via a computed string key', () => {
        const src = `$cheby(x, 3, { ['blockDC']: true })`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });

    // ─── Conservative skips ───────────────────────────────────────────────

    test('skips a config passed as a variable', () => {
        const src = `const cfg = { freq: 'c4' };\n$cheby(x, 3, cfg)`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped.length).toBe(1);
    });

    test('skips a config built with a spread', () => {
        const src = `$cheby(x, 3, { ...base, freq: 'c4' })`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped.length).toBe(1);
    });

    test('does not touch an unrelated .cheby() method call', () => {
        const src = `myThing.cheby(3)`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped).toEqual([]);
    });

    test('does not touch an unrelated $chebyshev call', () => {
        const src = `$chebyshev(x, 3)`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });

    // ─── Multiple calls ───────────────────────────────────────────────────

    test('migrates several $cheby calls in one source', () => {
        const result = migrateChebyBlockDC(
            `$cheby(a, 1).out()\n$cheby(b, 2, { freq: 'c4' }).out()`,
        );
        expect(result.migrated).toBe(
            `$cheby(a, 1, { blockDC: false }).out()\n` +
                `$cheby(b, 2, { blockDC: false, freq: 'c4' }).out()`,
        );
        expect(result.callsChanged).toBe(2);
    });

    test('returns source unchanged when there are no $cheby calls', () => {
        const src = `$sine('c4').out()`;
        const result = migrateChebyBlockDC(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });
});
