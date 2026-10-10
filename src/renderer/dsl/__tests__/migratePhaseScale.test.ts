import { describe, expect, test } from 'vitest';

import { migratePhaseScale } from '../migratePhaseScale';

describe('migratePhaseScale', () => {
    test('scales a literal phaseOffset', () => {
        const r = migratePhaseScale(`$sine('c4', { phaseOffset: 0.25 }).out()`);
        expect(r.migrated).toBe(`$sine('c4', { phaseOffset: 1.25 }).out()`);
        expect(r.callsChanged).toBe(1);
        expect(r.skipped).toEqual([]);
    });

    test('scales phaseOffset on saw, pulse, wavetable and the chain form', () => {
        const r = migratePhaseScale(
            `$saw('c3', { phaseOffset: 0.5 });
$pulse('c3', { phaseOffset: 0.1 });
$wavetable(w, 'c3', { phaseOffset: -0.5 });
$sine('c3').$.saw('c3', { phaseOffset: 1 });`,
        );
        expect(r.migrated).toBe(
            `$saw('c3', { phaseOffset: 2.5 });
$pulse('c3', { phaseOffset: 0.5 });
$wavetable(w, 'c3', { phaseOffset: -2.5 });
$sine('c3').$.saw('c3', { phaseOffset: 5 });`,
        );
        expect(r.callsChanged).toBe(4);
    });

    test('rounds away float noise', () => {
        const r = migratePhaseScale(`$sine('c4', { phaseOffset: 0.07 })`);
        expect(r.migrated).toBe(`$sine('c4', { phaseOffset: 0.35 })`);
    });

    test('scales a slider and its range', () => {
        const r = migratePhaseScale(
            `$sine('c4', { phaseOffset: $slider('phaseOffset', 0, 0, 1) })`,
        );
        expect(r.migrated).toBe(
            `$sine('c4', { phaseOffset: $slider('phaseOffset', 0, 0, 5) })`,
        );
    });

    test('scales groups and arrays of literals', () => {
        const r = migratePhaseScale(
            `$sine($g1(['c3', 'e3']), { phaseOffset: $g2([0, 0.25]) })`,
        );
        expect(r.migrated).toBe(
            `$sine($g1(['c3', 'e3']), { phaseOffset: $g2([0, 1.25]) })`,
        );
    });

    test('reports a phaseOffset that is not a literal', () => {
        const src = `$sine('c4', { phaseOffset: offset })`;
        const r = migratePhaseScale(src);
        expect(r.migrated).toBe(src);
        expect(r.callsChanged).toBe(0);
        expect(r.skipped).toHaveLength(1);
    });

    test('leaves phaseOffset on other modules alone', () => {
        const src = `foo('c4', { phaseOffset: 0.25 })`;
        expect(migratePhaseScale(src).migrated).toBe(src);
    });

    test('scales a literal phase input but not a signal one', () => {
        const r = migratePhaseScale(
            `$pSine(0.25).out(); $crush($ramp('c3'), 2).out(); $pSaw(0.5)`,
        );
        expect(r.migrated).toBe(
            `$pSine(1.25).out(); $crush($ramp('c3'), 2).out(); $pSaw(2.5)`,
        );
        expect(r.skipped).toEqual([]);
    });

    test('scales the first argument of a table warp', () => {
        const r = migratePhaseScale(
            `$table.mirror(0.5, $table.bend(-0.3, $table.pwm(0.4)))`,
        );
        expect(r.migrated).toBe(
            `$table.mirror(2.5, $table.bend(-1.5, $table.pwm(2)))`,
        );
        expect(r.callsChanged).toBe(3);
    });

    test('reports a table warp driven by a signal', () => {
        const r = migratePhaseScale(`$table.fold($sine('1hz'))`);
        expect(r.callsChanged).toBe(0);
        expect(r.skipped).toHaveLength(1);
    });

    test('reports a $ramp read raw but not one read through range or a phase module', () => {
        const r = migratePhaseScale(
            `const a = $ramp('c3');
$pSine($crush($ramp('c3'), 2));
$ramp('1hz').range(0, 1);
$ramp('c3').$.crush(2);
$sine('c4', { phaseOffset: $ramp('1hz') });
$slewTarget($ramp('c4'));`,
        );
        expect(r.skipped).toHaveLength(2);
        expect(r.skipped[0]).toContain('line 1');
        expect(r.skipped[1]).toContain('line 6');
    });

    test('leaves source without phase values untouched', () => {
        const src = `$sine('c4').out()`;
        const r = migratePhaseScale(src);
        expect(r.migrated).toBe(src);
        expect(r.callsChanged).toBe(0);
        expect(r.skipped).toEqual([]);
    });
});
