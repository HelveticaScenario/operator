import { describe, expect, test } from 'vitest';

import { migrateAdsrDefaults } from '../migrateAdsrDefaults';

const FULL = `attack: 0.01, decay: 0.1, sustain: 5, release: 0.1, curve: 0`;

describe('migrateAdsrDefaults', () => {
    // ─── Direct calls ─────────────────────────────────────────────────────

    test('appends the full default set to a bare $adsr(gate) call', () => {
        const result = migrateAdsrDefaults(`$adsr(gate).out()`);
        expect(result.migrated).toBe(`$adsr(gate, { ${FULL} }).out()`);
        expect(result.callsChanged).toBe(1);
        expect(result.skipped).toEqual([]);
    });

    test('fills an empty config object cleanly', () => {
        const result = migrateAdsrDefaults(`$adsr(gate, {}).out()`);
        expect(result.migrated).toBe(`$adsr(gate, { ${FULL} }).out()`);
        expect(result.callsChanged).toBe(1);
    });

    test('injects only the missing keys, keeping explicit ones', () => {
        const result = migrateAdsrDefaults(
            `$adsr(gate, { attack: 0.02 }).out()`,
        );
        expect(result.migrated).toBe(
            `$adsr(gate, { decay: 0.1, sustain: 5, release: 0.1, curve: 0, attack: 0.02 }).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('pins sustain when decay is set but sustain is omitted', () => {
        // The new engine drops sustain to 0 in this case; pinning sustain: 5
        // restores the old full-sustain default.
        const result = migrateAdsrDefaults(`$adsr(gate, { decay: 0.3 })`);
        expect(result.migrated).toContain('sustain: 5');
        expect(result.migrated).toContain('decay: 0.3');
        expect(result.migrated).not.toContain('decay: 0.1');
    });

    // ─── Dollar-chain form ────────────────────────────────────────────────

    test('injects into a .$.adsr(config) chain call', () => {
        const result = migrateAdsrDefaults(`gate.$.adsr({ decay: 0.2 }).out()`);
        expect(result.migrated).toBe(
            `gate.$.adsr({ attack: 0.01, sustain: 5, release: 0.1, curve: 0, decay: 0.2 }).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('adds config to a bare .$.adsr() chain call', () => {
        const result = migrateAdsrDefaults(`gate.$.adsr().out()`);
        expect(result.migrated).toBe(`gate.$.adsr({ ${FULL} }).out()`);
        expect(result.callsChanged).toBe(1);
    });

    // ─── Mix-chain form (.$m injects a leading mix arg) ───────────────────

    test('injects past the mix arg in a .$m.adsr(mix, config) call', () => {
        const result = migrateAdsrDefaults(
            `gate.$m.adsr(2.5, { decay: 0.2 }).out()`,
        );
        expect(result.migrated).toBe(
            `gate.$m.adsr(2.5, { attack: 0.01, sustain: 5, release: 0.1, curve: 0, decay: 0.2 }).out()`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('adds config after the mix arg in a .$m.adsr(mix) call', () => {
        const result = migrateAdsrDefaults(`gate.$m.adsr(2.5).out()`);
        expect(result.migrated).toBe(`gate.$m.adsr(2.5, { ${FULL} }).out()`);
        expect(result.callsChanged).toBe(1);
    });

    // ─── Idempotency ──────────────────────────────────────────────────────

    test('is a no-op when all five keys are already set', () => {
        const src = `$adsr(gate, { ${FULL} }).out()`;
        const result = migrateAdsrDefaults(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
    });

    test('running twice changes nothing the second time', () => {
        const once = migrateAdsrDefaults(`$adsr(gate)`).migrated;
        const twice = migrateAdsrDefaults(once).migrated;
        expect(twice).toBe(once);
    });

    // ─── Conservative skips ───────────────────────────────────────────────

    test('skips a config passed as a variable', () => {
        const result = migrateAdsrDefaults(`$adsr(gate, opts).out()`);
        expect(result.migrated).toBe(`$adsr(gate, opts).out()`);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped).toHaveLength(1);
    });

    test('skips a config built with a spread', () => {
        const result = migrateAdsrDefaults(
            `$adsr(gate, { ...opts, decay: 0.2 })`,
        );
        expect(result.callsChanged).toBe(0);
        expect(result.skipped).toHaveLength(1);
    });

    test('leaves an unrelated .adsr() method untouched', () => {
        const src = `myThing.adsr({ foo: 1 })`;
        const result = migrateAdsrDefaults(src);
        expect(result.migrated).toBe(src);
        expect(result.callsChanged).toBe(0);
        expect(result.skipped).toEqual([]);
    });
});
