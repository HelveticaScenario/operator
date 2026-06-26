import { describe, expect, it } from 'vitest';
import { MIGRATIONS, NEXT_VERSION, migrationsNeededFor } from './registry';

const ids = (evaluatedVersion: string | undefined) =>
    migrationsNeededFor(evaluatedVersion).map((m) => m.id);

describe('migration registry', () => {
    it('has unique ids and unique, ascending order keys', () => {
        const idSet = new Set(MIGRATIONS.map((m) => m.id));
        expect(idSet.size).toBe(MIGRATIONS.length);
        const orders = MIGRATIONS.map((m) => m.order);
        expect(new Set(orders).size).toBe(orders.length);
        expect([...orders]).toEqual([...orders].sort((a, b) => a - b));
    });

    it('marks each sinceVersion as semver or the unreleased sentinel', () => {
        for (const m of MIGRATIONS) {
            expect(
                m.sinceVersion === NEXT_VERSION ||
                    /^\d+\.\d+\.\d+$/.test(m.sinceVersion),
            ).toBe(true);
        }
    });
});

describe('migrationsNeededFor', () => {
    it('offers every migration to a never-evaluated patch, in order', () => {
        expect(ids(undefined)).toEqual([
            'cycle-to-pattern',
            'wavetable-pitch-first',
            'cheby-block-dc',
            'adsr-legacy-defaults',
        ]);
    });

    it('skips migrations the patch last evaluated after', () => {
        // 0.0.70 is past cycle (0.0.68) but before wavetable (0.0.97).
        expect(ids('0.0.70')).toEqual([
            'wavetable-pitch-first',
            'cheby-block-dc',
            'adsr-legacy-defaults',
        ]);
        // 0.0.97 conforms to wavetable; the two 0.0.102 changes still apply.
        expect(ids('0.0.97')).toEqual([
            'cheby-block-dc',
            'adsr-legacy-defaults',
        ]);
    });

    it('offers nothing to a patch evaluated at or after the newest change', () => {
        expect(ids('0.0.102')).toEqual([]);
        expect(ids('9.9.9')).toEqual([]);
    });
});
