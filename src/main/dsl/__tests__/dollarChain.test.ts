/**
 * Tests for the `.$.` and `.$m.` chainable module namespaces.
 *
 * `.$.` injects the receiving output as a module's first (signal) argument
 * ($sine(0).$.lpf('100hz') ≡ $lpf($sine(0), '100hz')); `.$m.` does the same but
 * crossfades the dry input against the wet result by a leading `mix` signal.
 */

import { beforeEach, describe, expect, test } from 'vitest';
import type { PatchGraph } from '@modular/core';
import schemas from '@modular/core/schemas.json';
import { DSLContext } from '../factories';
import {
    Collection,
    type GraphBuilder,
    ModuleOutput,
} from '../GraphBuilder';
import { executePatchScript } from '../executor';
import {
    dollarMethodName,
    processModuleSchema,
    qualifiesForDollarChain,
} from '../paramsSchema';

const EXECUTION_OPTIONS = {
    sampleRate: 48_000,
    workspaceRoot: '/workspace',
} as const;

function execPatch(source: string): PatchGraph {
    return executePatchScript(source, schemas, EXECUTION_OPTIONS).patch;
}

/** Recursively drop `__argument_spans` (which differ by source text, not graph). */
function stripSpans(value: unknown): unknown {
    if (Array.isArray(value)) {
        return value.map(stripSpans);
    }
    if (value && typeof value === 'object') {
        const out: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(value)) {
            if (k === '__argument_spans') {
                continue;
            }
            out[k] = stripSpans(v);
        }
        return out;
    }
    return value;
}

/** Normalize a patch to its modules (id, type, span-free params), sorted by id. */
function normalize(patch: PatchGraph) {
    return patch.modules
        .map((m) => ({
            id: m.id,
            moduleType: m.moduleType,
            params: stripSpans(m.params),
        }))
        .sort((a, b) => a.id.localeCompare(b.id));
}

function moduleTypes(patch: PatchGraph): string[] {
    return patch.modules.map((m) => m.moduleType);
}

let builder: GraphBuilder;

beforeEach(() => {
    builder = new DSLContext(schemas).getBuilder();
});

// ─── `.$.` equivalence with the bare factory ─────────────────────────────────

describe('.$. chainable namespace', () => {
    test('$sine(0).$.lpf("100hz") is identical to $lpf($sine(0), "100hz")', () => {
        const viaDollar = execPatch('$sine(0).$.lpf("100hz").out()');
        const viaFactory = execPatch('$lpf($sine(0), "100hz").out()');
        expect(normalize(viaDollar)).toEqual(normalize(viaFactory));
    });

    test('injects the receiver as the first argument ($lpf created, wired)', () => {
        const patch = execPatch('$sine(0).$.lpf("100hz").out()');
        expect(moduleTypes(patch)).toContain('$lpf');
        expect(moduleTypes(patch)).toContain('$sine');
    });

    test('multi-output module exposes its extra outputs (.low/.mid/.high)', () => {
        const sig = builder.getFactory('$sine')(0);
        const xover = (sig as unknown as { $: Record<string, () => unknown> }).$
            .xover() as Record<string, unknown>;
        expect(xover).toHaveProperty('low');
        expect(xover).toHaveProperty('mid');
        expect(xover).toHaveProperty('high');
    });

    test('unknown methods, symbols, and `then` resolve to undefined', () => {
        const sig = builder.getFactory('$sine')(0);
        const dollar = (sig as unknown as { $: Record<PropertyKey, unknown> }).$;
        expect(dollar.notAModule).toBeUndefined();
        expect(dollar[Symbol.iterator]).toBeUndefined();
        // `then` undefined keeps the proxy from being mistaken for a thenable.
        expect(dollar.then).toBeUndefined();
    });

    test('empty collection yields an empty Collection without throwing', () => {
        const result = (
            new Collection() as unknown as { $: Record<string, (...a: unknown[]) => unknown> }
        ).$.lpf('100hz');
        expect(result).toBeInstanceOf(Collection);
        expect((result as Collection).items.length).toBe(0);
    });
});

// ─── `.$m.` crossfade ────────────────────────────────────────────────────────

describe('.$m. mix namespace', () => {
    test('$saw(0).$m.lpf(2.5, "100hz") equals .pipeMix(s => $lpf(s, "100hz"), 2.5)', () => {
        const viaDollarMix = execPatch('$saw(0).$m.lpf(2.5, "100hz").out()');
        const viaPipeMix = execPatch(
            '$saw(0).pipeMix((s) => $lpf(s, "100hz"), 2.5).out()',
        );
        expect(normalize(viaDollarMix)).toEqual(normalize(viaPipeMix));
    });

    test('crossfade emits $mix, $remap, $clamp, and $scaleAndShift', () => {
        const types = moduleTypes(execPatch('$saw(0).$m.lpf(2.5, "100hz").out()'));
        expect(types).toContain('$mix');
        expect(types).toContain('$remap');
        expect(types).toContain('$clamp');
        expect(types).toContain('$scaleAndShift');
    });
});

// ─── pipeMix regression: ModuleOutput.pipeMix now crossfades ─────────────────

describe('ModuleOutput.pipeMix regression', () => {
    test('produces a $remap/$clamp crossfade, not a bare 2-input $mix', () => {
        const out = new ModuleOutput(builder, 'src-1', 'out', 0);
        out.pipeMix(
            (s) => builder.getFactory('$lpf')(s, '100hz') as Collection,
        );
        const types = builder.toPatch().modules.map((m) => m.moduleType);
        expect(types).toContain('$remap');
        expect(types).toContain('$clamp');
        expect(types).toContain('$mix');
    });
});

// ─── drift guard: runtime set === type-gen set ───────────────────────────────

describe('dollar-chain drift guard', () => {
    test('runtime dollarLookup matches the type-generation qualifying set exactly', () => {
        const dollarLookup = (
            builder as unknown as { dollarLookup: Map<string, string> }
        ).dollarLookup;
        const runtimeNames = [...dollarLookup.keys()].sort();

        // Mirror generateDSL: userFacing schemas, the same predicate, the same
        // name derivation (dollarMethodName).
        const typeGenNames = schemas
            .filter((s) => s.name !== '_clock' && s.name !== '$buffer')
            .filter((s) =>
                qualifiesForDollarChain(
                    processModuleSchema(
                        s as unknown as Parameters<typeof processModuleSchema>[0],
                    ),
                ),
            )
            .map((s) => dollarMethodName(s.name))
            .sort();

        // Set-equality, not just counts: catches a name the runtime would
        // register but type generation would drop (or vice versa).
        expect(runtimeNames).toEqual(typeGenNames);
        expect(runtimeNames.length).toBeGreaterThan(0);
        // Every emitted name must be a bare identifier — it is a TS interface member.
        for (const name of runtimeNames) {
            expect(name).toMatch(/^[$A-Za-z_][$A-Za-z0-9_]*$/);
        }
    });
});
