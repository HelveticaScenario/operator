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
import { Collection, type GraphBuilder, ModuleOutput } from '../GraphBuilder';
import { executePatchScript } from '../executor';
import { processModuleSchema, qualifiesForDollarChain } from '../paramsSchema';

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
        const xover = (
            sig as unknown as { $: Record<string, () => unknown> }
        ).$.xover() as Record<string, unknown>;
        expect(xover).toHaveProperty('low');
        expect(xover).toHaveProperty('mid');
        expect(xover).toHaveProperty('high');
    });

    test('unknown methods, symbols, and `then` resolve to undefined', () => {
        const sig = builder.getFactory('$sine')(0);
        const dollar = (sig as unknown as { $: Record<PropertyKey, unknown> })
            .$;
        expect(dollar.notAModule).toBeUndefined();
        expect(dollar[Symbol.iterator]).toBeUndefined();
        // `then` undefined keeps the proxy from being mistaken for a thenable.
        expect(dollar.then).toBeUndefined();
    });

    test('empty collection yields an empty Collection without throwing', () => {
        const result = (
            new Collection() as unknown as {
                $: Record<string, (...a: unknown[]) => unknown>;
            }
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
        const types = moduleTypes(
            execPatch('$saw(0).$m.lpf(2.5, "100hz").out()'),
        );
        expect(types).toContain('$mix');
        expect(types).toContain('$remap');
        expect(types).toContain('$clamp');
        expect(types).toContain('$scaleAndShift');
    });
});

// ─── synthetic dollar methods (DSL sugar registered via registerDollarMethod) ─

describe('synthetic dollar methods ($delay)', () => {
    test('$sine(0).$.delay(0.25) is identical to $delay($sine(0), 0.25)', () => {
        const viaDollar = execPatch('$sine(0).$.delay(0.25).out()');
        const viaFactory = execPatch('$delay($sine(0), 0.25).out()');
        expect(normalize(viaDollar)).toEqual(normalize(viaFactory));
    });

    test('.$.delay forwards options through to $delay', () => {
        const viaDollar = execPatch(
            '$sine(0).$.delay(0.25, { feedback: 4, maxTime: 2 }).out()',
        );
        const viaFactory = execPatch(
            '$delay($sine(0), 0.25, { feedback: 4, maxTime: 2 }).out()',
        );
        expect(normalize(viaDollar)).toEqual(normalize(viaFactory));
    });

    test('.$m.delay crossfades the dry signal against the wet delay', () => {
        const types = moduleTypes(
            execPatch('$sine(0).$m.delay(2.5, 0.25).out()'),
        );
        expect(types).toContain('$delayRead'); // wet delay tap
        expect(types).toContain('$remap'); // crossfade dry leg
        expect(types).toContain('$scaleAndShift');
    });
});

// ─── nested `.$.unstable.shape.*` chain ──────────────────────────────────────

describe('namespaced dollar chain', () => {
    test('.$.unstable.shape.<leaf> equals the bare $unstable.shape factory', () => {
        const viaChain = execPatch(
            "$sine(0).$.unstable.shape.fold('dual', 2).out()",
        );
        const viaFactory = execPatch(
            "$unstable.shape.fold($sine(0), 'dual', 2).out()",
        );
        expect(normalize(viaChain)).toEqual(normalize(viaFactory));
        expect(moduleTypes(viaChain)).toContain('$unstable.shape.fold');
    });

    test('leaf collisions are namespaced: $unstable.shape.fold coexists with $fold', () => {
        // `.$.unstable.shape.fold` resolves to the waveshaper…
        expect(
            moduleTypes(
                execPatch("$sine(0).$.unstable.shape.fold('dual', 2).out()"),
            ),
        ).toContain('$unstable.shape.fold');
        // …while the flat `.$.fold` still resolves to the fx wavefolder.
        expect(moduleTypes(execPatch('$sine(0).$.fold(2).out()'))).toContain(
            '$fold',
        );
    });

    test('.$m.unstable.shape.<leaf> crossfades dry against the shaped wet', () => {
        const types = moduleTypes(
            execPatch(
                "$sine(0).$m.unstable.shape.saturate(2.5, 'hard', 3).out()",
            ),
        );
        expect(types).toContain('$unstable.shape.saturate');
        expect(types).toContain('$remap'); // crossfade dry leg
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
    test('runtime dollar chain matches the type-generation qualifying set exactly', () => {
        interface Node {
            leaves: Map<string, string>;
            children: Map<string, Node>;
        }
        const b = builder as unknown as {
            dollarLookup: Map<string, string>;
            dollarNamespaceRoot: Node;
        };
        // Flat leaves plus full dotted `namespace…leaf` paths, walking the tree.
        const collectPaths = (node: Node, prefix: string): string[] => {
            const out: string[] = [];
            for (const leaf of node.leaves.keys()) {
                out.push(prefix ? `${prefix}.${leaf}` : leaf);
            }
            for (const [seg, child] of node.children) {
                out.push(
                    ...collectPaths(child, prefix ? `${prefix}.${seg}` : seg),
                );
            }
            return out;
        };
        const runtimeNames = [
            ...b.dollarLookup.keys(),
            ...collectPaths(b.dollarNamespaceRoot, ''),
        ].sort();

        // Mirror generateDSL: a module's `.$.` access path is its name minus the
        // leading `$` (flat → the leaf; dotted → the full `namespace…leaf` path).
        const dollarPath = (name: string): string =>
            name.startsWith('$') ? name.slice(1) : name;
        const typeGenNames = schemas
            .filter((s) => s.name !== '_clock' && s.name !== '$buffer')
            .filter((s) =>
                qualifiesForDollarChain(
                    processModuleSchema(
                        s as unknown as Parameters<
                            typeof processModuleSchema
                        >[0],
                    ),
                ),
            )
            .map((s) => dollarPath(s.name))
            .sort();

        // Set-equality, not just counts: catches a name the runtime would
        // register but type generation would drop (or vice versa).
        expect(runtimeNames).toEqual(typeGenNames);
        expect(runtimeNames.length).toBeGreaterThan(0);
        // Every path segment must be a bare identifier — it is a TS interface member.
        for (const name of runtimeNames) {
            for (const segment of name.split('.')) {
                expect(segment).toMatch(/^[$A-Za-z_][$A-Za-z0-9_]*$/);
            }
        }
    });
});
