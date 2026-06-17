/**
 * Integration tests for the DSL executor pipeline.
 *
 * These tests exercise the full DSL → PatchGraph pipeline:
 *   schemas.json → executePatchScript(source, schemas) → PatchGraph
 *
 * No Electron, no audio hardware needed — runs in plain Node.js via Vitest.
 */

import { describe, expect, expectTypeOf, test } from 'vitest';
import type { PatchGraph } from '@modular/core';
import schemas from '@modular/core/schemas.json';
import { type DSLExecutionResult, executePatchScript } from '../executor';
import type { CollectionWithRange, ModuleOutputWithRange } from '../GraphBuilder';

const DEFAULT_EXECUTION_OPTIONS = {
    sampleRate: 48_000,
    workspaceRoot: '/workspace',
} as const;

// ─── Helpers ──────────────────────────────────────────────────────────────────

function exec(source: string): DSLExecutionResult {
    return executePatchScript(source, schemas, DEFAULT_EXECUTION_OPTIONS);
}

function execPatch(source: string): PatchGraph {
    return exec(source).patch;
}

/** Find a module by type in the patch (excluding built-in ROOT_CLOCK, ROOT_INPUT) */
function findModules(patch: PatchGraph, moduleType: string) {
    return patch.modules.filter((m) => m.moduleType === moduleType);
}

/** Count user-created modules (exclude well-known built-ins) */
function userModules(patch: PatchGraph) {
    const builtIns = new Set(['ROOT_CLOCK', 'ROOT_INPUT', 'ROOT_OUTPUT']);
    return patch.modules.filter((m) => !builtIns.has(m.id));
}

// ─── Schema loading ──────────────────────────────────────────────────────────

describe('schema loading', () => {
    test('schemas.json contains non-empty array', () => {
        expect(schemas.length).toBeGreaterThan(0);
    });

    test('schemas include core module types', () => {
        const names = schemas.map((s) => s.name);
        expect(names).toContain('$sine');
        expect(names).toContain('$saw');
        expect(names).toContain('$pulse');
        expect(names).toContain('$lpf');
        expect(names).toContain('$adsr');
        expect(names).toContain('_clock');
        expect(names).toContain('$mix');
    });
});

// ─── Basic oscillators ───────────────────────────────────────────────────────

describe('basic oscillators', () => {
    test('$sine with note string', () => {
        const patch = execPatch('$sine("C4").out()');
        const sines = findModules(patch, '$sine');
        expect(sines.length).toBe(1);
        expect(patch.scopes).toEqual([]); // No scope call
    });

    test('$sine with Hz string "440hz"', () => {
        const patch = execPatch('$sine("440hz").out()');
        const sines = findModules(patch, '$sine');
        expect(sines.length).toBe(1);
    });

    test('$sine with Hz string "440Hz" (capitalized)', () => {
        const patch = execPatch('$sine("440Hz").out()');
        const sines = findModules(patch, '$sine');
        expect(sines.length).toBe(1);
    });

    test('$sine with $hz() helper', () => {
        const patch = execPatch('$sine($hz(440)).out()');
        const sines = findModules(patch, '$sine');
        expect(sines.length).toBe(1);
    });

    test('$sine with MIDI note string "60m"', () => {
        const patch = execPatch('$sine("60m").out()');
        const sines = findModules(patch, '$sine');
        expect(sines.length).toBe(1);
    });

    test('$sine with raw number', () => {
        const patch = execPatch('$sine(0).out()');
        const sines = findModules(patch, '$sine');
        expect(sines.length).toBe(1);
    });

    test('$saw with shape config', () => {
        const patch = execPatch('$saw("A3", { shape: 2.5 }).out()');
        const saws = findModules(patch, '$saw');
        expect(saws.length).toBe(1);
    });

    test('$pulse with width config', () => {
        const patch = execPatch('$pulse("C4", { width: 1.0 }).out()');
        const pulses = findModules(patch, '$pulse');
        expect(pulses.length).toBe(1);
    });

    test('$noise with color param', () => {
        const patch = execPatch('$noise("white").out()');
        const noises = findModules(patch, '$noise');
        expect(noises.length).toBe(1);
    });
});

// ─── Signal input variants equivalence ───────────────────────────────────────

describe('signal input variants', () => {
    test('"440hz" and "440Hz" both produce valid patches', () => {
        const patchLower = execPatch('$sine("440hz").out()');
        const patchUpper = execPatch('$sine("440Hz").out()');
        // Both should have a sine module
        expect(findModules(patchLower, '$sine').length).toBe(1);
        expect(findModules(patchUpper, '$sine').length).toBe(1);
    });

    test('decimal Hz string "261.63hz"', () => {
        const patch = execPatch('$sine("261.63hz").out()');
        expect(findModules(patch, '$sine').length).toBe(1);
    });

    test('$hz() helper produces a number', () => {
        // $hz returns a voltage value — test it via $sine
        const patch = execPatch('$sine($hz(261.63)).out()');
        expect(findModules(patch, '$sine').length).toBe(1);
    });

    test('$note() helper produces a number', () => {
        const patch = execPatch('$sine($note("A4")).out()');
        expect(findModules(patch, '$sine').length).toBe(1);
    });

    test('$setTempo() accepts plain BPM number', () => {
        const _patch = execPatch('$setTempo(140)');
        // Should not throw — $setTempo(140) sets tempo as plain BPM
    });

    test('scale pattern string produces polyphonic module', () => {
        const patch = execPatch('$sine("4s(C4:major)").out()');
        expect(findModules(patch, '$sine').length).toBe(1);
    });
});

// ─── Filters ─────────────────────────────────────────────────────────────────

describe('filters', () => {
    test('$lpf with collection input', () => {
        const patch = execPatch('$lpf($saw("C3"), "C5").out()');
        expect(findModules(patch, '$lpf').length).toBe(1);
        expect(findModules(patch, '$saw').length).toBe(1);
    });

    test('$hpf with Hz string cutoff', () => {
        const patch = execPatch('$hpf($noise("pink"), "1000hz").out()');
        expect(findModules(patch, '$hpf').length).toBe(1);
    });

    test('$bpf with resonance', () => {
        const patch = execPatch('$bpf($saw("C3"), "C5", 4).out()');
        expect(findModules(patch, '$bpf').length).toBe(1);
    });

    test('$lpf with $hz cutoff', () => {
        const patch = execPatch('$lpf($noise("white"), $hz(1000)).out()');
        expect(findModules(patch, '$lpf').length).toBe(1);
    });
});

// ─── Envelopes ───────────────────────────────────────────────────────────────

describe('envelopes', () => {
    test('$adsr with gate input and config', () => {
        const patch = execPatch(
            '$adsr($clock.beatTrigger, { attack: 0.1, decay: 0.2, sustain: 3, release: 0.5 }).out()',
        );
        expect(findModules(patch, '$adsr').length).toBe(1);
    });

    test('$perc with trigger', () => {
        const patch = execPatch(
            '$perc($clock.beatTrigger, { decay: 0.3 }).out()',
        );
        expect(findModules(patch, '$perc').length).toBe(1);
    });
});

// ─── Polyphony ───────────────────────────────────────────────────────────────

describe('polyphony', () => {
    test('array of notes creates polyphonic module', () => {
        const patch = execPatch('$sine(["C3", "E3", "G3"]).out()');
        expect(findModules(patch, '$sine').length).toBe(1);
    });

    test('polyphonic filter', () => {
        const patch = execPatch('$lpf($saw(["C3", "E3"]), "C5").out()');
        expect(findModules(patch, '$lpf').length).toBe(1);
        expect(findModules(patch, '$saw').length).toBe(1);
    });
});

// ─── Collections ─────────────────────────────────────────────────────────────

describe('collections', () => {
    test('$c spreads collections into a new collection', () => {
        const patch = execPatch(
            '$c(...$sine("C4"), ...$saw("E4")).amplitude(0.5).out()',
        );
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$saw').length).toBe(1);
    });

    test('$r spreads ranged collections', () => {
        const patch = execPatch(
            '$r(...$sine("C4"), ...$saw("E4")).range(0, 1).out()',
        );
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$saw').length).toBe(1);
    });

    test('$c with noise (ModuleOutputWithRange, no spread needed)', () => {
        const patch = execPatch('$c($noise("white"), $noise("pink")).out()');
        expect(findModules(patch, '$noise').length).toBe(2);
    });

    test('collection indexing', () => {
        const patch = execPatch('$sine("C4")[0].out()');
        expect(findModules(patch, '$sine').length).toBe(1);
    });
});

// ─── Mixing ──────────────────────────────────────────────────────────────────

describe('mixing', () => {
    test('$mix with array of collections', () => {
        const patch = execPatch('$mix([$sine("C4"), $saw("E4")]).out()');
        // .out() also creates a $mix in the output chain, so expect ≥ 2
        expect(findModules(patch, '$mix').length).toBeGreaterThanOrEqual(2);
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$saw').length).toBe(1);
    });

    test('$mix with mode config', () => {
        const patch = execPatch(
            '$mix([$sine("C4"), $saw("E4")], { mode: "average" }).out()',
        );
        expect(findModules(patch, '$mix').length).toBeGreaterThanOrEqual(2);
    });

    test('$stereoMix', () => {
        const patch = execPatch(
            '$stereoMix($sine(["C3", "E3", "G3"]), { width: 5 }).out()',
        );
        // .out() also creates a $stereoMix in the output chain, so expect ≥ 2
        expect(findModules(patch, '$stereoMix').length).toBeGreaterThanOrEqual(
            2,
        );
    });
});

// ─── Chaining ────────────────────────────────────────────────────────────────

describe('chaining methods', () => {
    test('.amplitude() creates a scaleAndShift module', () => {
        const patch = execPatch('$sine("C4").amplitude(0.5).out()');
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$scaleAndShift').length).toBeGreaterThan(0);
    });

    test('.shift() creates a scaleAndShift module', () => {
        const patch = execPatch('$sine("C4").shift(2.5).out()');
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$scaleAndShift').length).toBeGreaterThan(0);
    });

    test('.gain() creates curve and scaleAndShift modules', () => {
        const patch = execPatch('$sine("C4").gain(2.5).out()');
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$curve').length).toBeGreaterThan(0);
        expect(findModules(patch, '$scaleAndShift').length).toBeGreaterThan(0);
    });

    test('.exp() creates a curve module', () => {
        const patch = execPatch('$sine("C4").exp(2).out()');
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$curve').length).toBeGreaterThan(0);
    });

    test('.exp() with default factor creates a curve module', () => {
        const patch = execPatch('$sine("C4").exp().out()');
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$curve').length).toBeGreaterThan(0);
    });

    test('.scope() adds a scope entry', () => {
        const patch = execPatch('$sine("C4").scope().out()');
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(patch.scopes.length).toBeGreaterThan(0);
        expect(patch.scopes[0].channels).toBeDefined();
        expect(patch.scopes[0].channels.length).toBe(1);
        expect(patch.scopes[0].channels[0].channel).toBe(0);
    });

    test('.scope() with config', () => {
        const patch = execPatch(
            '$sine("C4").scope({ msPerFrame: 100, range: [-10, 10] }).out()',
        );
        expect(patch.scopes.length).toBeGreaterThan(0);
        const scope = patch.scopes[0];
        expect(scope.msPerFrame).toBe(100);
        expect(scope.range).toEqual([-10, 10]);
        expect(scope.channels.length).toBe(1);
    });

    test('.scope() on collection captures all channels', () => {
        const patch = execPatch('$sine(["C4", "E4"]).scope().out()');
        expect(patch.scopes.length).toBe(1);
        expect(patch.scopes[0].channels.length).toBe(2);
        expect(patch.scopes[0].channels[0].channel).toBe(0);
        expect(patch.scopes[0].channels[1].channel).toBe(1);
    });

    test('.scope() on indexed output captures single channel', () => {
        const patch = execPatch('$sine(["C4", "E4"])[1].scope().out()');
        expect(patch.scopes.length).toBe(1);
        expect(patch.scopes[0].channels.length).toBe(1);
        expect(patch.scopes[0].channels[0].channel).toBe(1);
    });

    test('ModuleOutputWithRange.range() remaps', () => {
        const patch = execPatch('$sine("C4")[0].range("C3", "C5").out()');
        // Range() on a ModuleOutputWithRange creates a remap module
        expect(findModules(patch, '$sine').length).toBe(1);
        expect(findModules(patch, '$remap').length).toBeGreaterThan(0);
    });

    test('dynamicRange output wires .range() through virtual rangeMin / rangeMax cables', () => {
        // $clamp declares dynamic_range, so .range(0, 5) should bind the
        // remap's inMin / inMax to the upstream's virtual range ports rather
        // than baking in the static [-5, 5].
        const patch = execPatch(
            '$clamp($sine("C4"), { min: -2, max: 3 }).range(0, 5).out()',
        );
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBeGreaterThan(0);

        const params = remaps[0].params as Record<string, unknown>;

        const cableOf = (v: unknown): Record<string, unknown> | undefined => {
            if (Array.isArray(v)) {
                return v[0] as Record<string, unknown>;
            }
            if (v && typeof v === 'object') {
                return v as Record<string, unknown>;
            }
            return undefined;
        };

        const inMin = cableOf(params.inMin);
        const inMax = cableOf(params.inMax);
        expect(inMin).toBeDefined();
        expect(inMax).toBeDefined();
        expect(inMin?.type).toBe('cable');
        expect(inMax?.type).toBe('cable');
        expect(inMin?.port).toBe('output.rangeMin');
        expect(inMax?.port).toBe('output.rangeMax');
    });

    test('static-range output keeps numeric inMin / inMax on .range()', () => {
        // $sine has a static range — no dynamic_range, so .range() should
        // still pass numeric bounds straight into the remap.
        const patch = execPatch('$sine("C4").range(0, 1).out()');
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBeGreaterThan(0);
        const params = remaps[0].params as Record<string, unknown>;
        // Numeric (or constant Volts wrapped) — definitely not a cable to a
        // rangeMin / rangeMax port.
        const inMin = params.inMin;
        const inMax = params.inMax;
        const looksLikeCable = (v: unknown) =>
            v &&
            typeof v === 'object' &&
            !Array.isArray(v) &&
            (v as Record<string, unknown>).type === 'cable';
        expect(looksLikeCable(inMin)).toBe(false);
        expect(looksLikeCable(inMax)).toBe(false);
    });

    test('.range() return type is CollectionWithRange (type-level)', () => {
        // The remap produced by .range() is itself range-aware, so both
        // .range() overloads must declare CollectionWithRange as their return
        // type. Enforced by `tsc`; a no-op at runtime.
        expectTypeOf<
            ReturnType<ModuleOutputWithRange['range']>
        >().toEqualTypeOf<CollectionWithRange>();
        expectTypeOf<
            ReturnType<CollectionWithRange['range']>
        >().toEqualTypeOf<CollectionWithRange>();
    });

    test('.range() returns a range-aware collection: chaining .range().range() wires the second remap to the first remap virtual range ports', () => {
        // $remap's output is itself a dynamic_range output, so the result of
        // .range(...) must be a CollectionWithRange whose .range(outMin, outMax)
        // re-binds to the upstream remap's virtual range ports — not the 4-arg
        // Collection.range that would leave inMin / inMax unset.
        const patch = execPatch('$clamp($sine("C4"), { min: -2, max: 3 }).range(0, 5).range(0, 1).out()');
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBe(2);

        const remapIds = new Set(remaps.map((m) => m.id));
        const cableOf = (v: unknown): Record<string, unknown> | undefined => {
            if (Array.isArray(v)) return v[0] as Record<string, unknown>;
            if (v && typeof v === 'object') return v as Record<string, unknown>;
            return undefined;
        };

        // The chained (outer) remap is the one whose inMin / inMax cables point
        // at the other remap's virtual range ports.
        const chained = remaps.find((m) => {
            const p = m.params as Record<string, unknown>;
            const inMin = cableOf(p.inMin);
            return (
                inMin?.type === 'cable' &&
                inMin?.port === 'output.rangeMin' &&
                remapIds.has(inMin?.module as string)
            );
        });
        expect(chained).toBeDefined();
        const cp = chained!.params as Record<string, unknown>;
        expect(cableOf(cp.inMax)?.port).toBe('output.rangeMax');
        expect(remapIds.has(cableOf(cp.inMax)?.module as string)).toBe(true);
    });
});

// ─── Modulation routing ──────────────────────────────────────────────────────

describe('modulation routing', () => {
    test('LFO modulating oscillator pitch', () => {
        const source = `
            const lfo = $sine($hz(2))
            $sine(lfo.amplitude(1).shift(0)).out()
        `;
        const patch = execPatch(source);
        // Two sine modules: one as LFO, one as audio oscillator
        expect(findModules(patch, '$sine').length).toBe(2);
    });

    test('subtractive synth voice (osc → env → filter)', () => {
        const source = `
            const osc = $saw("C3")
            const env = $adsr($clock.beatTrigger, { attack: 0.01, decay: 0.3, sustain: 2, release: 0.5 })
            $lpf(osc, env.range("C3", "C6")).out()
        `;
        const patch = execPatch(source);
        expect(findModules(patch, '$saw').length).toBe(1);
        expect(findModules(patch, '$adsr').length).toBe(1);
        expect(findModules(patch, '$lpf').length).toBe(1);
    });
});

// ─── Sequencing & patterns ───────────────────────────────────────────────────

describe('sequencing', () => {
    test('$cycle with $p() pattern', () => {
        const patch = execPatch('$cycle($p("c4 e4 g4 b4")).out()');
        expect(findModules(patch, '$cycle').length).toBe(1);
    });

    test('$track with keyframes', () => {
        const patch = execPatch('$track([[$hz(440), 0], [$hz(880), 1]]).out()');
        expect(findModules(patch, '$track').length).toBe(1);
    });

    test('$cycle($p.s(...)) builds a scale-degree pattern', () => {
        const single = execPatch('$cycle($p.s("0 2 4 5 7", "C(major)")).out()');
        expect(findModules(single, '$cycle').length).toBe(1);
        // Chained .add folds a second source additively.
        const chained = execPatch(
            '$cycle($p.s("0 2 4", "C(major)").add("0 3")).out()',
        );
        expect(findModules(chained, '$cycle').length).toBe(1);
    });

    test('$p rejects dropped atom kinds', () => {
        expect(() => execPatch('$p("m60")')).toThrow();
        expect(() => execPatch('$p("bd sd")')).toThrow();
        expect(() => execPatch('$p("module(osc1:out:0)")')).toThrow();
        expect(() => execPatch('$p("2v")')).toThrow();
    });

    test('$p.s rejects non-integer atoms at patch-graph validation', () => {
        expect(() =>
            execPatch('$cycle($p.s("1.5", "C(major)")).out()'),
        ).toThrow(/IntervalValue requires integer scale degrees, got 1\.5/);
        expect(() =>
            execPatch('$cycle($p.s("c4", "C(major)")).out()'),
        ).toThrow(/IntervalValue does not accept note atoms/);
        expect(() =>
            execPatch('$cycle($p.s("440hz", "C(major)")).out()'),
        ).toThrow(/IntervalValue does not accept Hz atoms/);
    });

    test('$cycle accepts mixed numeric, note, and hz atoms', () => {
        const patch = execPatch('$cycle($p("0.5 c4 440hz -1")).out()');
        expect(findModules(patch, '$cycle').length).toBe(1);
    });

    test('$p.s(...).sub(...) wire payload preserves chain RHS argument_spans[1]', () => {
        // Regression: the chain RHS span must be captured by the analyzer
        // and carried through to the SpPattern wire payload as
        // argument_spans[1], pointing at the literal '0 5' in the user
        // source so editor highlighting can follow it.
        const source =
            "const pat = $p.s('0 1 2 3', 'c(maj)').sub('0 5')\n" +
            'const seq = $cycle(pat)\n' +
            'seq.out()';
        const patch = execPatch(source);
        const cycles = findModules(patch, '$cycle');
        expect(cycles.length).toBe(1);

        // The SpPattern lives on $cycle.pattern as an opaque wire payload.
        const pattern = cycles[0].params.pattern as {
            __kind: string;
            sources: Array<{ source: string }>;
            ops: Array<{ op: string; mode: string }>;
            argument_spans: Array<{ start: number; end: number }>;
        };
        expect(pattern.__kind).toBe('SpPattern');
        expect(pattern.sources.length).toBe(2);
        expect(pattern.sources[1].source).toBe('0 5');
        expect(pattern.ops).toEqual([{ op: 'sub', mode: 'in' }]);

        // argument_spans must be parallel to sources: one per source.
        expect(pattern.argument_spans.length).toBe(2);

        // argument_spans[1] should bracket the '0 5' literal in the
        // original source string (including surrounding quotes is fine
        // either way as long as the substring it points at contains
        // '0 5').
        const rhsSpan = pattern.argument_spans[1];
        expect(rhsSpan).toBeDefined();
        expect(typeof rhsSpan.start).toBe('number');
        expect(typeof rhsSpan.end).toBe('number');
        expect(rhsSpan.end).toBeGreaterThan(rhsSpan.start);

        // The span must NOT be the {0, 0} fallback used when the
        // analyzer fails to locate the chain RHS.
        expect(rhsSpan).not.toEqual({ start: 0, end: 0 });

        // The slice of the source the span points at should contain
        // the literal RHS pattern characters '0 5'.
        const slice = source.slice(rhsSpan.start, rhsSpan.end);
        expect(slice.includes('0 5')).toBe(true);
    });

    test('$p.s chain on a const-bound pattern captures RHS argument_spans', () => {
        // Regression: chain ops (.add/.sub/...) applied to a pattern stored
        // in a const variable must still resolve their chain root back to
        // `$p.s(...)` so the analyzer registers the RHS literal span. Before
        // the fix only inline `$p.s(...).add(...)` chains were tracked; a
        // `const p = $p.s(...)` followed by `p.add('0 5')` fell back to the
        // {0,0} sentinel and produced no editor highlight.
        const source =
            "const p1 = $p.s('0 1 2 3', 'c(maj)')\n" +
            "const p2 = p1.add('0,2')\n" +
            'const seq = $cycle(p2)\n' +
            'seq.out()';
        const patch = execPatch(source);
        const cycles = findModules(patch, '$cycle');
        expect(cycles.length).toBe(1);

        const pattern = cycles[0].params.pattern as {
            __kind: string;
            sources: Array<{ source: string }>;
            argument_spans: Array<{ start: number; end: number }>;
        };
        expect(pattern.__kind).toBe('SpPattern');
        expect(pattern.sources.length).toBe(2);
        expect(pattern.argument_spans.length).toBe(2);

        const rhsSpan = pattern.argument_spans[1];
        expect(rhsSpan).not.toEqual({ start: 0, end: 0 });
        expect(rhsSpan.end).toBeGreaterThan(rhsSpan.start);
        expect(source.slice(rhsSpan.start, rhsSpan.end).includes('0,2')).toBe(
            true,
        );
    });

    test('$p.s nested chain on a const-bound pattern captures every RHS span', () => {
        // Multi-link chain (`.sub(...).add.squeeze(...)`) rooted at a const
        // pattern, applied inline inside `$cycle`. Every chained RHS literal
        // must get a real span, not the {0,0} fallback.
        const source =
            "const p1 = $p.s('0 1 2 3', 'c(maj)')\n" +
            "const seq = $cycle(p1.sub('7').add.squeeze('{0 1 2 3}%2'))\n" +
            'seq.out()';
        const patch = execPatch(source);
        const cycles = findModules(patch, '$cycle');
        expect(cycles.length).toBe(1);

        const pattern = cycles[0].params.pattern as {
            __kind: string;
            sources: Array<{ source: string }>;
            argument_spans: Array<{ start: number; end: number }>;
        };
        expect(pattern.__kind).toBe('SpPattern');
        expect(pattern.sources.length).toBe(3);
        expect(pattern.argument_spans.length).toBe(3);

        const subSpan = pattern.argument_spans[1];
        const squeezeSpan = pattern.argument_spans[2];
        for (const span of [subSpan, squeezeSpan]) {
            expect(span).not.toEqual({ start: 0, end: 0 });
            expect(span.end).toBeGreaterThan(span.start);
        }
        expect(source.slice(subSpan.start, subSpan.end).includes('7')).toBe(
            true,
        );
        expect(
            source
                .slice(squeezeSpan.start, squeezeSpan.end)
                .includes('{0 1 2 3}%2'),
        ).toBe(true);
    });

    test('$p.s chain rooted in a const-of-const resolves through both', () => {
        // The chain root walk must recurse through multiple const hops:
        // p2 derives from p1, p3 chains off p2.
        const source =
            "const p1 = $p.s('0 1 2 3', 'c(maj)')\n" +
            "const p2 = p1.add('0,2')\n" +
            "const p3 = p2.sub('1')\n" +
            'const seq = $cycle(p3)\n' +
            'seq.out()';
        const patch = execPatch(source);
        const cycles = findModules(patch, '$cycle');
        expect(cycles.length).toBe(1);

        const pattern = cycles[0].params.pattern as {
            argument_spans: Array<{ start: number; end: number }>;
        };
        expect(pattern.argument_spans.length).toBe(3);
        const lastSpan = pattern.argument_spans[2];
        expect(lastSpan).not.toEqual({ start: 0, end: 0 });
        expect(source.slice(lastSpan.start, lastSpan.end).includes('1')).toBe(
            true,
        );
    });

    test('const-bound chain populates $cycle __argument_spans per source', () => {
        // End-to-end: the renderer highlights by combining the module's
        // `__argument_spans['pattern.<i>']` (document offsets) with the
        // Rust-emitted `param_spans['pattern.<i>']`. Before the fix, only
        // `pattern.0` was emitted for a const-bound chain, so only the first
        // source highlighted. Assert every source's offset is present and
        // brackets the right literal.
        const source =
            "const p1 = $p.s('0 1 2 3', 'c(maj)')\n" +
            "const seq = $cycle(p1.sub('7').add.squeeze('{0 1 2 3}%2'))\n" +
            'seq.out()';
        const patch = execPatch(source);
        const cycle = findModules(patch, '$cycle')[0];
        const argSpans = (
            cycle.params as {
                __argument_spans?: Record<string, { start: number; end: number }>;
            }
        ).__argument_spans;
        expect(argSpans).toBeDefined();

        const expected: Record<string, string> = {
            'pattern.0': '0 1 2 3',
            'pattern.1': '7',
            'pattern.2': '{0 1 2 3}%2',
        };
        for (const [key, literal] of Object.entries(expected)) {
            const span = argSpans?.[key];
            expect(span, `missing ${key}`).toBeDefined();
            expect(source.slice(span!.start, span!.end).includes(literal)).toBe(
                true,
            );
        }
    });

    test('$p.s accepts a reassigned string variable as source', () => {
        // The inline migration form `$cycle($p.s(pat, key))` (used when an
        // $iCycle source variable feeds calls with conflicting scales) keeps
        // `pat` a raw string, so $p.s must consume the variable's runtime
        // (last-assigned) value.
        const source =
            "const key = 'c(maj)'\n" +
            "let pat = '<0 2 4>*16'\n" +
            "pat = '<0 2 <4!2 5>>*16'\n" +
            'const seq = $cycle($p.s(pat, key))\n' +
            'seq.out()';
        const patch = execPatch(source);
        const cycles = findModules(patch, '$cycle');
        expect(cycles.length).toBe(1);

        const pattern = cycles[0].params.pattern as {
            __kind: string;
            sources: Array<{ source: string }>;
        };
        expect(pattern.__kind).toBe('SpPattern');
        // The last assignment wins — $p.s parsed the reassigned value.
        expect(pattern.sources[0].source).toBe('<0 2 <4!2 5>>*16');
    });
});

// ─── Utilities ───────────────────────────────────────────────────────────────

describe('utilities', () => {
    test('$remap', () => {
        const patch = execPatch('$remap($sine("C4"), 0, 1, -5, 5).out()');
        expect(findModules(patch, '$remap').length).toBe(1);
    });

    test('$scaleAndShift', () => {
        const patch = execPatch('$scaleAndShift($sine("C4"), 0.5, 2.5).out()');
        expect(findModules(patch, '$scaleAndShift').length).toBeGreaterThan(0);
    });

    test('$curve', () => {
        const patch = execPatch('$curve($sine("C4"), 2).out()');
        expect(findModules(patch, '$curve').length).toBeGreaterThan(0);
    });

    test('$sah (sample and hold)', () => {
        const patch = execPatch(
            '$sah($noise("white"), $clock.beatTrigger).out()',
        );
        expect(findModules(patch, '$sah').length).toBe(1);
    });

    test('$slew', () => {
        const patch = execPatch(
            '$slew($clock.beatTrigger, { rise: 0.01, fall: 0.01 }).out()',
        );
        expect(findModules(patch, '$slew').length).toBe(1);
    });

    test('$dcBlock', () => {
        const patch = execPatch('$dcBlock($pulse("C2", { width: 1 })).out()');
        expect(findModules(patch, '$dcBlock').length).toBe(1);
    });

    test('$quantizer', () => {
        const patch = execPatch('$quantizer($sine("C4"), "C(major)").out()');
        expect(findModules(patch, '$quantizer').length).toBe(1);
    });

    test('$clockDivider', () => {
        const patch = execPatch('$clockDivider($clock.beatTrigger, 4).out()');
        expect(findModules(patch, '$clockDivider').length).toBe(1);
    });

    test('$math expression', () => {
        const patch = execPatch(
            '$math("sin(x * 3.14159)", { x: $sine("C4")[0] }).out()',
        );
        expect(findModules(patch, '$math').length).toBe(1);
    });

    test('$bufRead', () => {
        const patch = execPatch(
            'const buf = $buffer($sine("C4"), 0.25)\n$bufRead(buf, 0).out()',
        );
        expect(findModules(patch, '$bufRead').length).toBe(1);
        expect(findModules(patch, '$buffer').length).toBe(1);
    });
});

// ─── Deferred / feedback ─────────────────────────────────────────────────────

describe('deferred signals', () => {
    test('$deferred creates placeholder', () => {
        const source = `
            const fb = $deferred()
            const sig = $slew(fb[0], { rise: 0.01, fall: 0.01 })
            fb.set(sig)
            sig.out()
        `;
        const patch = execPatch(source);
        expect(findModules(patch, '$slew').length).toBe(1);
    });

    test('$deferred with multiple channels', () => {
        const source = `
            const fb = $deferred(2)
            fb.set($sine(["C4", "E4"]))
            fb.out()
        `;
        const patch = execPatch(source);
        expect(findModules(patch, '$sine').length).toBe(1);
    });
});

// ─── Slider ──────────────────────────────────────────────────────────────────

describe('sliders', () => {
    test('$slider creates a signal module and returns slider def', () => {
        const result = exec(
            'const vol = $slider("Volume", 0.5, 0, 1)\n$sine("C4").amplitude(vol).out()',
        );
        expect(result.sliders.length).toBe(1);
        expect(result.sliders[0].label).toBe('Volume');
        expect(result.sliders[0].value).toBe(0.5);
        expect(result.sliders[0].min).toBe(0);
        expect(result.sliders[0].max).toBe(1);
    });

    test('$slider duplicate label throws', () => {
        expect(() =>
            execPatch(`
                $slider("Freq", 440, 20, 20000)
                $slider("Freq", 880, 20, 20000)
            `),
        ).toThrow('unique');
    });

    test('$slider().range(outMin, outMax) infers input bounds from the slider', () => {
        // $slider carries a static [min, max] range, so the 2-arg .range()
        // should bake the slider's own 0 / 1 into the remap as the input
        // bounds — as constants, never as cables to virtual range ports.
        const patch = execPatch('$slider("X", 0.5, 0, 1).range(0, 100).out()');
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBe(1);

        const params = remaps[0].params as Record<string, unknown>;
        const isCable = (v: unknown) =>
            !!v &&
            typeof v === 'object' &&
            !Array.isArray(v) &&
            (v as Record<string, unknown>).type === 'cable';
        const scalarOf = (v: unknown): unknown =>
            Array.isArray(v) ? v[0] : v;

        expect(isCable(params.inMin)).toBe(false);
        expect(isCable(params.inMax)).toBe(false);
        expect(scalarOf(params.inMin)).toBe(0); // slider min
        expect(scalarOf(params.inMax)).toBe(1); // slider max
    });

    test('$slider().range(outMin, outMax, inMin, inMax) override beats the declared bounds', () => {
        // The 4-arg form must override the slider's declared 0 / 10 input
        // bounds with the supplied 2 / 8.
        const patch = execPatch(
            '$slider("Y", 5, 0, 10).range(0, 1, 2, 8).out()',
        );
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBe(1);

        const params = remaps[0].params as Record<string, unknown>;
        const scalarOf = (v: unknown): unknown =>
            Array.isArray(v) ? v[0] : v;

        expect(scalarOf(params.inMin)).toBe(2); // override, not 0
        expect(scalarOf(params.inMax)).toBe(8); // override, not 10
    });

    test('$slider().range() override honors an explicit 0 (nullish, not falsy)', () => {
        // The override is nullish (`??`), so passing inMin = 0 must be honored
        // rather than falling back to the slider's declared min of 1. Under a
        // `||` fallback this would wrongly resolve to 1.
        const patch = execPatch(
            '$slider("Z", 5, 1, 10).range(0, 1, 0, 8).out()',
        );
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBe(1);

        const params = remaps[0].params as Record<string, unknown>;
        const scalarOf = (v: unknown): unknown => (Array.isArray(v) ? v[0] : v);

        expect(scalarOf(params.inMin)).toBe(0); // explicit 0 honored, not slider min 1
        expect(scalarOf(params.inMax)).toBe(8); // override
    });
});

// ─── Dynamic-range .range() wiring ────────────────────────────────────────────

describe('dynamic-range .range()', () => {
    // A dynamic-range bound is wired as a cable to a virtual range port and may
    // arrive array-wrapped (`[cable]`) from the polyphonic remap; unwrap first.
    const cableOf = (v: unknown): Record<string, unknown> | undefined => {
        const inner = Array.isArray(v) ? v[0] : v;
        return inner && typeof inner === 'object'
            ? (inner as Record<string, unknown>)
            : undefined;
    };
    const isCableVal = (v: unknown): boolean => cableOf(v)?.type === 'cable';
    const scalarOf = (v: unknown): unknown => (Array.isArray(v) ? v[0] : v);

    test('a dynamic-range output wires virtual rangeMin/rangeMax cables into $remap', () => {
        // $clamp declares dynamic_range, so the 2-arg .range() must wire cables
        // to its virtual output.rangeMin / output.rangeMax ports rather than
        // bake in the static -5 / 5 fallback constants.
        const patch = execPatch(
            "$clamp($sine('c3'), { min: -2, max: 3 }).range(0, 1).out()",
        );
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBe(1);

        const params = remaps[0].params as Record<string, unknown>;
        expect(isCableVal(params.inMin)).toBe(true);
        expect(isCableVal(params.inMax)).toBe(true);
        expect(cableOf(params.inMin)?.port).toBe('output.rangeMin');
        expect(cableOf(params.inMax)?.port).toBe('output.rangeMax');
    });

    test('a single-bound override replaces only that bound; the other stays a virtual-port cable', () => {
        // .range(0, 1, 0) overrides inMin with the explicit scalar 0 (nullish,
        // so honored) while inMax keeps wiring the live output.rangeMax cable.
        const patch = execPatch(
            "$clamp($sine('c3'), { min: -2, max: 3 }).range(0, 1, 0).out()",
        );
        const remaps = findModules(patch, '$remap');
        expect(remaps.length).toBe(1);

        const params = remaps[0].params as Record<string, unknown>;
        expect(isCableVal(params.inMin)).toBe(false);
        expect(scalarOf(params.inMin)).toBe(0); // explicit 0 honored
        expect(isCableVal(params.inMax)).toBe(true);
        expect(cableOf(params.inMax)?.port).toBe('output.rangeMax');
    });
});

// ─── Global settings ─────────────────────────────────────────────────────────

describe('global settings', () => {
    test('$setTempo does not throw', () => {
        expect(() => execPatch('$setTempo(140)')).not.toThrow();
    });

    test('$setOutputGain does not throw', () => {
        expect(() => execPatch('$setOutputGain(5.0)')).not.toThrow();
    });
});

// ─── Built-in modules ────────────────────────────────────────────────────────

describe('built-in modules', () => {
    test('$clock is available and has outputs', () => {
        // Use $clock outputs as trigger input to an envelope
        const patch = execPatch(
            '$adsr($clock.beatTrigger, { attack: 0.01, decay: 0.1, sustain: 3, release: 0.2 }).out()',
        );
        expect(patch.modules.find((m) => m.id === 'ROOT_CLOCK')).toBeDefined();
    });

    test('$clock.beatTrigger can modulate another module', () => {
        const patch = execPatch(
            '$adsr($clock.beatTrigger, { attack: 0.01, decay: 0.1, sustain: 3, release: 0.2 }).out()',
        );
        expect(patch.modules.find((m) => m.id === 'ROOT_CLOCK')).toBeDefined();
        expect(findModules(patch, '$adsr').length).toBe(1);
    });

    test('$input is available', () => {
        const patch = execPatch('$input[0].out()');
        expect(patch.modules.find((m) => m.id === 'ROOT_INPUT')).toBeDefined();
    });
});

// ─── FX modules ──────────────────────────────────────────────────────────────

describe('fx modules', () => {
    test('$crush', () => {
        const patch = execPatch('$crush($sine("C4"), 3).out()');
        expect(findModules(patch, '$crush').length).toBe(1);
    });

    test('$fold', () => {
        const patch = execPatch('$fold($sine("C4"), 3).out()');
        expect(findModules(patch, '$fold').length).toBe(1);
    });

    test('$cheby', () => {
        const patch = execPatch('$cheby($sine("C4"), 3).out()');
        expect(findModules(patch, '$cheby').length).toBe(1);
    });

    test('$comp accepts ratio < 1 for expansion', () => {
        const source = `
            // ratio 0.5 = upward expansion above threshold (boost loud)
            // upwardRatio 0.5 = downward expansion below threshold (gate)
            $comp($saw("C3"), {
                threshold: 1.0, ratio: 0.5,
                upwardThreshold: 0.3, upwardRatio: 0.5,
            }).out()
        `;
        const patch = execPatch(source);
        expect(findModules(patch, '$comp').length).toBe(1);
    });

    test('$comp accepts sidechain + upward params', () => {
        const source = `
            const kick = $sine("C2")
            const pad = $saw("A3")
            $comp(pad, {
                sidechain: kick,
                threshold: 1.0, ratio: 8,
                upwardThreshold: 0.5, upwardRatio: 4,
                attack: 0.005, release: 0.2,
            }).out()
        `;
        const patch = execPatch(source);
        const comps = findModules(patch, '$comp');
        expect(comps.length).toBe(1);
        // Sidechain param should be wired up as a signal cable.
        expect(comps[0].params.sidechain).toBeDefined();
        expect(comps[0].params.upwardThreshold).toBeDefined();
        expect(comps[0].params.upwardRatio).toBeDefined();
    });

    test('$ott builds 3-band split + 3 compressors + crossfade', () => {
        const patch = execPatch('$ott($saw("C3")).out()');
        // One $xover splits, three $comp instances run per band.
        expect(findModules(patch, '$xover').length).toBe(1);
        expect(findModules(patch, '$comp').length).toBe(3);
    });

    test('$ott with sidechain splits sidechain through second xover', () => {
        const source = `
            const kick = $sine("C2")
            $ott($saw("C3"), { sidechain: kick }).out()
        `;
        const patch = execPatch(source);
        // Two $xover instances: one for input, one for sidechain.
        expect(findModules(patch, '$xover').length).toBe(2);
        const comps = findModules(patch, '$comp');
        expect(comps.length).toBe(3);
        // Every band-compressor must have a sidechain wired up.
        for (const c of comps) {
            expect(c.params.sidechain).toBeDefined();
        }
    });

    test('$ott honors custom config', () => {
        const source = `
            $ott($saw("C3"), {
                depth: 4,
                lowMidFreq: $hz(150),
                midHighFreq: $hz(3000),
                lowGain: 6, midGain: 5, highGain: 4,
            }).out()
        `;
        const patch = execPatch(source);
        expect(findModules(patch, '$xover').length).toBe(1);
        expect(findModules(patch, '$comp').length).toBe(3);
    });
});

// ─── Complex patches ─────────────────────────────────────────────────────────

describe('complex patches', () => {
    test('multi-voice FM synth', () => {
        const source = `
            const notes = ["C3", "E3", "G3"]
            const mod = $sine($hz(3))
            const carrier = $sine(notes)
            $lpf(carrier, mod.range("C4", "C6"), 2).out()
        `;
        const patch = execPatch(source);
        expect(findModules(patch, '$sine').length).toBe(2);
        expect(findModules(patch, '$lpf').length).toBe(1);
    });

    test('sequenced subtractive synth', () => {
        const source = `
            const seq = $cycle($p("c3 e3 g3 b3"))
            const osc = $saw(seq)
            const env = $adsr($clock.beatTrigger, { attack: 0.01, decay: 0.2, sustain: 2, release: 0.3 })
            $lpf(osc, env.range("C3", "C6")).out()
        `;
        const patch = execPatch(source);
        expect(findModules(patch, '$cycle').length).toBe(1);
        expect(findModules(patch, '$saw').length).toBe(1);
        expect(findModules(patch, '$adsr').length).toBe(1);
        expect(findModules(patch, '$lpf').length).toBe(1);
    });
});

// ─── Error cases ─────────────────────────────────────────────────────────────

describe('error handling', () => {
    test('empty source produces a valid (minimal) patch', () => {
        const patch = execPatch('');
        // Should have at least the built-in modules
        expect(patch.modules.length).toBeGreaterThan(0);
    });

    test('syntax error in DSL throws', () => {
        expect(() => execPatch('$sine((')).toThrow();
    });

    test('undefined function throws', () => {
        expect(() => execPatch('$unknownModule("C4").out()')).toThrow();
    });

    test('runtime error throws with DSL prefix', () => {
        expect(() => execPatch('null.out()')).toThrow();
    });

    test('missing required param throws with module name, line, and param name', () => {
        expect(() => execPatch('$lpf()')).toThrow(
            '$lpf at line 1: missing required parameter `input`',
        );
    });

    test('providing required param does not throw', () => {
        expect(() => execPatch('$lpf($sine("C4"), "C4").out()')).not.toThrow();
    });
});

// ─── Pipe vs direct call comparison ──────────────────────────────────────────

describe('pipe vs direct call', () => {
    test('pipe $lpf produces same $lpf params as direct call', () => {
        const directPatch = execPatch('$lpf($saw("c"), "1000hz").out()');
        const pipePatch = execPatch(
            '$saw("c").pipe(e => $lpf(e, "1000hz")).out()',
        );

        const directLpf = findModules(directPatch, '$lpf')[0];
        const pipeLpf = findModules(pipePatch, '$lpf')[0];

        // Compare params excluding __argument_spans (source positions differ)
        const { __argument_spans: _a, ...directCore } = directLpf.params;
        const { __argument_spans: _b, ...pipeCore } = pipeLpf.params;

        // The $lpf params should be identical (input and cutoff)
        expect(pipeCore).toEqual(directCore);
    });

    test('pipe and direct produce identical full patch structure', () => {
        const directPatch = execPatch('$lpf($saw("c"), "1000hz").out()');
        const pipePatch = execPatch(
            '$saw("c").pipe(e => $lpf(e, "1000hz")).out()',
        );

        // Compare user modules
        const directUser = userModules(directPatch);
        const pipeUser = userModules(pipePatch);

        // Same number of modules
        expect(pipeUser.length).toEqual(directUser.length);
    });

    test('pipe $lpf does not throw', () => {
        expect(() =>
            execPatch('$saw("c").pipe(e => $lpf(e, "1000hz")).out()'),
        ).not.toThrow();
    });

    test('$saw direct out produces a valid patch', () => {
        const patch = execPatch('$saw("c").out()');
        const saws = findModules(patch, '$saw');
        expect(saws.length).toBe(1);
    });
});

describe('$buffer()', () => {
    test('creates a buffer module and returns a buffer_ref', () => {
        expect(() =>
            exec(`
                const buf = $buffer($sine("C4"), 0.5);
                if (buf.type !== 'buffer_ref') {
                    throw new Error('expected type "buffer_ref", got ' + buf.type);
                }
                if (buf.frameCount !== 24000) {
                    throw new Error('expected frameCount 24000, got ' + String(buf.frameCount));
                }
                if (buf.port !== 'buffer') {
                    throw new Error('expected port "buffer", got ' + buf.port);
                }
            `),
        ).not.toThrow();
    });

    test('creates $buffer module in the patch graph', () => {
        const patch = execPatch(
            'const buf = $buffer($sine("C4"), 1)\n$bufRead(buf, 0).out()',
        );
        expect(findModules(patch, '$buffer').length).toBe(1);
    });

    test('rejects non-positive lengthSeconds', () => {
        expect(() =>
            executePatchScript(
                '$buffer($sine("C4"), 0)',
                schemas,
                DEFAULT_EXECUTION_OPTIONS,
            ),
        ).toThrow(/lengthSeconds must be greater than 0/);
    });

    test('passes config.id to the $buffer module', () => {
        const patch = execPatch(
            'const buf = $buffer($sine("C4"), 0.5, { id: "myBuf" })\n$bufRead(buf, 0).out()',
        );
        const buffers = findModules(patch, '$buffer');
        expect(buffers.length).toBe(1);
        expect(buffers[0].id).toBe('myBuf');
    });

    test('handles polyphonic input', () => {
        const patch = execPatch(
            'const buf = $buffer($sine(["C4", "E4"]), 0.5)\n$bufRead(buf, 0).out()',
        );
        expect(findModules(patch, '$buffer').length).toBe(1);
        expect(findModules(patch, '$bufRead').length).toBe(1);
    });

    test('rejects NaN lengthSeconds', () => {
        expect(() => execPatch('$buffer($sine("C4"), NaN)')).toThrow(
            /lengthSeconds must be a finite number/,
        );
    });

    test('rejects Infinity lengthSeconds', () => {
        expect(() => execPatch('$buffer($sine("C4"), Infinity)')).toThrow(
            /lengthSeconds must be a finite number/,
        );
    });

    test('$buffer with $delayRead creates both modules', () => {
        const patch = execPatch(
            'const buf = $buffer($sine("C4"), 0.5)\n$delayRead(buf, 0.1).out()',
        );
        expect(findModules(patch, '$buffer').length).toBe(1);
        expect(findModules(patch, '$delayRead').length).toBe(1);
    });

    test('sets length param on the $buffer module', () => {
        const patch = execPatch(
            'const buf = $buffer($sine("C4"), 0.25)\n$bufRead(buf, 0).out()',
        );
        const buffers = findModules(patch, '$buffer');
        expect(buffers[0].params.length).toBe(0.25);
    });
});

// ─── $wavs / $sampler ────────────────────────────────────────────────────────

describe('$wavs() and $sampler', () => {
    const wavsFolderTree = { kick: 'file', tables: { boom: 'file' } } as const;
    const loadWav = (path: string) => ({
        channels: path === 'kick' ? 1 : 2,
        frameCount: 1000,
        path,
        sampleRate: 44100,
        duration: 1000 / 44100,
        bitDepth: 16,
        pitch: path === 'kick' ? 0.0 : null,
        playback: path === 'kick' ? 'one-shot' : null,
        bpm: null,
        beats: null,
        timeSignature: null,
        loops: [],
        cuePoints: [],
        mtime: 1_700_000_000_000,
    });

    function execWithWavs(source: string) {
        return executePatchScript(source, schemas, {
            ...DEFAULT_EXECUTION_OPTIONS,
            wavsFolderTree: wavsFolderTree as any,
            loadWav,
        });
    }

    test('$wavs() returns wav_ref for known files', () => {
        const result = execWithWavs('$sampler($wavs().kick, 5).out()');
        const sampler = findModules(result.patch, '$sampler');
        expect(sampler.length).toBe(1);
        expect(sampler[0].params.wav).toMatchObject({
            type: 'wav_ref',
            path: 'kick',
            channels: 1,
            sampleRate: 44100,
            frameCount: 1000,
            bitDepth: 16,
            pitch: 0.0,
            playback: 'one-shot',
        });
        expect(sampler[0].params.wav.loops).toEqual([]);
        expect(sampler[0].params.wav.cuePoints).toEqual([]);
    });

    test('$wavs() traverses nested directories', () => {
        const result = execWithWavs('$sampler($wavs().tables.boom, 5).out()');
        const sampler = findModules(result.patch, '$sampler');
        expect(sampler.length).toBe(1);
        expect(sampler[0].params.wav).toMatchObject({
            type: 'wav_ref',
            path: 'tables/boom',
            channels: 2,
            sampleRate: 44100,
        });
    });

    test('$wavs() throws for missing files', () => {
        expect(() => execWithWavs('$wavs().snare')).toThrow(/not found/);
    });

    test('$wavs() numeric index returns lex-sorted file', () => {
        // Tree adds two top-level files alongside `kick` so the lex order is
        // deterministic: a, kick, z (subfolder `tables` excluded from index).
        const tree = {
            a: 'file',
            kick: 'file',
            z: 'file',
            tables: { boom: 'file' },
        } as const;
        const run = (src: string) =>
            executePatchScript(src, schemas, {
                ...DEFAULT_EXECUTION_OPTIONS,
                wavsFolderTree: tree as any,
                loadWav,
            });
        const r0 = run('$sampler($wavs()[0], 5).out()');
        expect(findModules(r0.patch, '$sampler')[0].params.wav.path).toBe('a');
        const r1 = run('$sampler($wavs()[1], 5).out()');
        expect(findModules(r1.patch, '$sampler')[0].params.wav.path).toBe(
            'kick',
        );
        const r2 = run('$sampler($wavs()[2], 5).out()');
        expect(findModules(r2.patch, '$sampler')[0].params.wav.path).toBe('z');
    });

    test('$wavs() numeric index wraps modulo file count', () => {
        const tree = { a: 'file', b: 'file', c: 'file' } as const;
        const run = (src: string) =>
            executePatchScript(src, schemas, {
                ...DEFAULT_EXECUTION_OPTIONS,
                wavsFolderTree: tree as any,
                loadWav,
            });
        // 3 files: positive wrap
        expect(
            findModules(
                run('$sampler($wavs()[3], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('a');
        expect(
            findModules(
                run('$sampler($wavs()[4], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('b');
        expect(
            findModules(
                run('$sampler($wavs()[5], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('c');
        // negative wrap
        expect(
            findModules(
                run('$sampler($wavs()[-1], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('c');
        expect(
            findModules(
                run('$sampler($wavs()[-2], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('b');
        expect(
            findModules(
                run('$sampler($wavs()[-3], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('a');
        expect(
            findModules(
                run('$sampler($wavs()[-4], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('c');
    });

    test('$wavs() numeric index works on subfolders', () => {
        const tree = {
            kick: 'file',
            drums: { hat: 'file', snare: 'file' },
        } as const;
        const run = (src: string) =>
            executePatchScript(src, schemas, {
                ...DEFAULT_EXECUTION_OPTIONS,
                wavsFolderTree: tree as any,
                loadWav,
            });
        expect(
            findModules(
                run('$sampler($wavs().drums[0], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('drums/hat');
        expect(
            findModules(
                run('$sampler($wavs().drums[1], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('drums/snare');
        expect(
            findModules(
                run('$sampler($wavs().drums[2], 5).out()').patch,
                '$sampler',
            )[0].params.wav.path,
        ).toBe('drums/hat');
    });

    test('$wavs() numeric index throws on folder with no direct files', () => {
        // `parent` has only a subfolder, no direct files — numeric index has
        // nothing to wrap into and must throw.
        const tree = { parent: { sub: { kick: 'file' } } } as const;
        const run = () =>
            executePatchScript('$wavs().parent[0]', schemas, {
                ...DEFAULT_EXECUTION_OPTIONS,
                wavsFolderTree: tree as any,
                loadWav,
            });
        expect(run).toThrow(/no wav files/);
    });

    test('$wavs() throws when no wavs/ folder', () => {
        expect(() =>
            executePatchScript('$wavs().kick', schemas, {
                ...DEFAULT_EXECUTION_OPTIONS,
                wavsFolderTree: null,
            }),
        ).toThrow(/no wavs\/ folder/);
    });

    test('$sampler with speed param produces correct patch', () => {
        const result = execWithWavs(
            '$sampler($wavs().kick, $pulse("4hz"), { speed: 0.5 }).out()',
        );
        const sampler = findModules(result.patch, '$sampler');
        expect(sampler.length).toBe(1);
        expect(sampler[0].params.wav).toMatchObject({
            type: 'wav_ref',
            path: 'kick',
            channels: 1,
        });
        // speed param should be present
        expect(sampler[0].params.speed).toBe(0.5);
    });

    test('$sampler with stereo wav sets correct channel count', () => {
        const result = execWithWavs(
            '$sampler($wavs().tables.boom, $pulse("2hz")).out()',
        );
        const sampler = findModules(result.patch, '$sampler');
        expect(sampler.length).toBe(1);
        // tables/boom is a 2-channel file
        expect(sampler[0].params.wav.channels).toBe(2);
    });

    test('$sampler chained with amplitude and scope', () => {
        const result = execWithWavs(
            '$sampler($wavs().kick, $pulse("4hz")).amplitude(0.5).scope().out()',
        );
        const sampler = findModules(result.patch, '$sampler');
        expect(sampler.length).toBe(1);
        expect(result.patch.scopes.length).toBeGreaterThan(0);
    });

    test('$wavs() loadWav is called during execution', () => {
        const calls: string[] = [];
        const trackingLoadWav = (path: string) => {
            calls.push(path);
            return {
                channels: 1,
                frameCount: 500,
                path,
                sampleRate: 44100,
                duration: 500 / 44100,
                bitDepth: 16,
                pitch: null,
                playback: null,
                bpm: null,
                beats: null,
                timeSignature: null,
                loops: [],
                cuePoints: [],
                mtime: 1_700_000_000_000,
            };
        };
        executePatchScript('$sampler($wavs().kick, 5).out()', schemas, {
            ...DEFAULT_EXECUTION_OPTIONS,
            wavsFolderTree: wavsFolderTree as any,
            loadWav: trackingLoadWav,
        });
        expect(calls).toContain('kick');
    });

    test('$wavs() root is enumerable with Object.keys()', () => {
        // Use Object.keys() to discover available wavs and access by dynamic key
        // wavsFolderTree = { kick: 'file', tables: { boom: 'file' } }
        const result = execWithWavs(
            `
            const w = $wavs();
            const keys = Object.keys(w);
            // keys should include both 'kick' (file) and 'tables' (dir)
            if (keys.length !== 2) throw new Error('expected 2 keys, got ' + keys.length);
            if (!keys.includes('kick')) throw new Error('missing kick');
            if (!keys.includes('tables')) throw new Error('missing tables');
            // Access a file by dynamic key
            $sampler(w.kick, 5).out();
            `,
        );
        const samplers = findModules(result.patch, '$sampler');
        expect(samplers.length).toBe(1);
        expect(samplers[0].params.wav.path).toBe('kick');
    });

    test('$wavs() nested directories are enumerable', () => {
        // Enumerate keys of a subdirectory
        const result = execWithWavs(
            `
            const t = $wavs().tables;
            const keys = Object.keys(t);
            for (const k of keys) {
                $sampler(t[k], 5).out();
            }
            `,
        );
        const samplers = findModules(result.patch, '$sampler');
        expect(samplers.length).toBe(1);
        expect(samplers[0].params.wav.path).toBe('tables/boom');
    });

    test('$wavs() exposes metadata with loops and cue points', () => {
        const metadataLoadWav = (path: string) => ({
            channels: 1,
            frameCount: 44100,
            path,
            sampleRate: 44100,
            duration: 1.0,
            bitDepth: 24,
            pitch: 0.75,
            playback: 'loop' as const,
            bpm: 120.0,
            beats: 4,
            timeSignature: { num: 4, den: 4 },
            loops: [
                { loopType: 'forward', start: 0.0, end: 0.5 },
                { loopType: 'pingpong', start: 0.25, end: 0.75 },
            ],
            cuePoints: [
                { position: 0.0, label: 'Start' },
                { position: 0.5, label: 'Middle' },
            ],
            mtime: 1_700_000_000_000,
        });

        const result = executePatchScript(
            '$sampler($wavs().kick, 5).out()',
            schemas,
            {
                ...DEFAULT_EXECUTION_OPTIONS,
                wavsFolderTree: wavsFolderTree as any,
                loadWav: metadataLoadWav,
            },
        );
        const sampler = findModules(result.patch, '$sampler');
        const wav = sampler[0].params.wav;

        expect(wav.sampleRate).toBe(44100);
        expect(wav.duration).toBe(1.0);
        expect(wav.bitDepth).toBe(24);
        expect(wav.pitch).toBe(0.75);
        expect(wav.playback).toBe('loop');
        expect(wav.bpm).toBe(120.0);
        expect(wav.beats).toBe(4);
        expect(wav.timeSignature).toEqual({ num: 4, den: 4 });
        expect(wav.loops).toEqual([
            { type: 'forward', start: 0.0, end: 0.5 },
            { type: 'pingpong', start: 0.25, end: 0.75 },
        ]);
        expect(wav.cuePoints).toEqual([
            { position: 0.0, label: 'Start' },
            { position: 0.5, label: 'Middle' },
        ]);
    });
});

// ─── $table DSL helpers ──────────────────────────────────────────────────────

describe('$table helpers', () => {
    const wavsFolderTree = { wt: 'file' } as const;
    const loadWav = (path: string) => ({
        channels: 1,
        frameCount: 2048,
        path,
        sampleRate: 48000,
        duration: 2048 / 48000,
        bitDepth: 16,
        pitch: null,
        playback: null,
        bpm: null,
        beats: null,
        timeSignature: null,
        loops: [],
        cuePoints: [],
        mtime: 1_700_000_000_000,
    });

    function execWithWavs(source: string) {
        return executePatchScript(source, schemas, {
            ...DEFAULT_EXECUTION_OPTIONS,
            wavsFolderTree: wavsFolderTree as any,
            loadWav,
        });
    }

    test('$table.mirror serializes a numeric amount', () => {
        const patch = execWithWavs(
            '$wavetable($wavs().wt, 0, 0, { phase: $table.mirror(0.5) }).out()',
        ).patch;
        const wt = findModules(patch, '$wavetable');
        expect(wt.length).toBe(1);
        expect(wt[0].params.phase).toEqual({ type: 'mirror', amount: 0.5 });
    });

    test('$table.bend serializes a ModuleOutput as a cable reference', () => {
        const patch = execWithWavs(
            'const lfo = $sine(0)\n$wavetable($wavs().wt, 0, 0, { phase: $table.bend(lfo) }).out()',
        ).patch;
        const wt = findModules(patch, '$wavetable');
        expect(wt.length).toBe(1);
        const phase = wt[0].params.phase as {
            type: string;
            amount: unknown;
        };
        expect(phase.type).toBe('bend');
        const cables = Array.isArray(phase.amount)
            ? phase.amount
            : [phase.amount];
        expect(cables[0]).toMatchObject({
            type: 'cable',
            port: 'output',
        });
    });

    test('$table variants use camelCase tags matching Rust deserializer', () => {
        const patch = execWithWavs(
            `const a = $wavetable($wavs().wt, 0, 0, { phase: $table.sync(1) }).out()
             const b = $wavetable($wavs().wt, 0, 0, { phase: $table.fold(0.2) }).out()
             const c = $wavetable($wavs().wt, 0, 0, { phase: $table.pwm(0.5) }).out()`,
        ).patch;
        const tables = findModules(patch, '$wavetable').map(
            (m) => m.params.phase,
        );
        expect(tables).toEqual(
            expect.arrayContaining([
                { type: 'sync', ratio: 1 },
                { type: 'fold', amount: 0.2 },
                { type: 'pwm', width: 0.5 },
            ]),
        );
    });

    test('optional second param composes two tables into a pipe descriptor', () => {
        const patch = execWithWavs(
            `$wavetable($wavs().wt, 0, 0, {
                phase: $table.mirror(0.5, $table.bend(0.3))
            }).out()`,
        ).patch;
        const wt = findModules(patch, '$wavetable');
        expect(wt.length).toBe(1);
        expect(wt[0].params.phase).toEqual({
            type: 'pipe',
            first: { type: 'mirror', amount: 0.5 },
            second: { type: 'bend', amount: 0.3 },
        });
    });

    test('second param chains left-to-right: mirror -> bend -> fold', () => {
        const patch = execWithWavs(
            `$wavetable($wavs().wt, 0, 0, {
                phase: $table.mirror(0.5, $table.bend(0.3, $table.fold(0.2)))
            }).out()`,
        ).patch;
        const wt = findModules(patch, '$wavetable');
        expect(wt[0].params.phase).toEqual({
            type: 'pipe',
            first: { type: 'mirror', amount: 0.5 },
            second: {
                type: 'pipe',
                first: { type: 'bend', amount: 0.3 },
                second: { type: 'fold', amount: 0.2 },
            },
        });
    });

    test('.pipe passes table to closure and returns result', () => {
        const patch = execWithWavs(
            `$wavetable($wavs().wt, 0, 0, {
                phase: $table.mirror(0.5).pipe(t => t)
            }).out()`,
        ).patch;
        const wt = findModules(patch, '$wavetable');
        expect(wt[0].params.phase).toEqual({ type: 'mirror', amount: 0.5 });
    });

    test('.pipe closure can build a pipe descriptor', () => {
        const patch = execWithWavs(
            `$wavetable($wavs().wt, 0, 0, {
                phase: $table.mirror(0.5).pipe(t => $table.bend(0.3, t))
            }).out()`,
        ).patch;
        const wt = findModules(patch, '$wavetable');
        // t is mirror; $table.bend(0.3, t) = pipe(bend, mirror) — bend feeds into mirror
        expect(wt[0].params.phase).toEqual({
            type: 'pipe',
            first: { type: 'bend', amount: 0.3 },
            second: { type: 'mirror', amount: 0.5 },
        });
    });
});

// ─── $scopeXY (background Lissajous) ─────────────────────────────────────────

describe('$scopeXY', () => {
    test('records a single (x, y) pair with default range', () => {
        const patch = execPatch(`
            const a = $sine($hz(440))
            const b = $sine($hz(330))
            $scopeXY(a, b)
            a.out()
        `);
        expect(patch.scopeXy).toBeDefined();
        expect(patch.scopeXy!.pairs).toHaveLength(1);
        expect(patch.scopeXy!.pairs[0].x.portName).toBe('output');
        expect(patch.scopeXy!.pairs[0].y.portName).toBe('output');
        expect(patch.scopeXy!.xRange).toEqual([-5, 5]);
        expect(patch.scopeXy!.yRange).toEqual([-5, 5]);
    });

    test('cycles the shorter axis to match the longer one', () => {
        const patch = execPatch(`
            const a = $sine($hz(110))
            const b = $sine($hz(220))
            const c = $sine($hz(440))
            $scopeXY($c(a, b), c)
            a.out()
        `);
        expect(patch.scopeXy!.pairs).toHaveLength(2);
        // Both pairs share the same y module (cycling c with len 1)
        const yModules = patch.scopeXy!.pairs.map((p) => p.y.moduleId);
        expect(new Set(yModules).size).toBe(1);
    });

    test('cycles both ways when neither length divides the other', () => {
        const patch = execPatch(`
            const a = $sine($hz(110))
            const b = $sine($hz(220))
            const p = $sine($hz(330))
            const q = $sine($hz(440))
            const r = $sine($hz(550))
            $scopeXY($c(a, b), $c(p, q, r))
            a.out()
        `);
        expect(patch.scopeXy!.pairs).toHaveLength(3);
        // x: [a, b, a] (cycle), y: [p, q, r]
        const xs = patch.scopeXy!.pairs.map((p) => p.x.moduleId);
        const ys = patch.scopeXy!.pairs.map((p) => p.y.moduleId);
        expect(xs[0]).toBe(xs[2]);
        expect(xs[0]).not.toBe(xs[1]);
        expect(new Set(ys).size).toBe(3);
    });

    test('last call wins', () => {
        const patch = execPatch(`
            const a = $sine($hz(110))
            const b = $sine($hz(220))
            const c = $sine($hz(330))
            $scopeXY(a, b)
            $scopeXY(b, c)
            a.out()
        `);
        expect(patch.scopeXy!.pairs).toHaveLength(1);
        // second call's pair must be the visible one
        expect(patch.scopeXy!.pairs[0].x.moduleId).not.toBe(
            patch.scopeXy!.pairs[0].y.moduleId,
        );
    });

    test('custom xRange / yRange flow through to the patch', () => {
        const patch = execPatch(`
            const a = $sine($hz(440))
            $scopeXY(a, a, { xRange: [-1, 1], yRange: [0, 10] })
            a.out()
        `);
        expect(patch.scopeXy!.xRange).toEqual([-1, 1]);
        expect(patch.scopeXy!.yRange).toEqual([0, 10]);
    });

    test('empty x or y is a no-op', () => {
        const patch = execPatch(`
            const a = $sine($hz(440))
            $scopeXY([], a)
            a.out()
        `);
        expect(patch.scopeXy).toBeUndefined();
    });

    test('bad range throws', () => {
        expect(() =>
            execPatch(`
                const a = $sine($hz(440))
                $scopeXY(a, a, { xRange: [5, -5] })
                a.out()
            `),
        ).toThrow(/xRange/);
    });

    test('bad yRange throws', () => {
        expect(() =>
            execPatch(`
                const a = $sine($hz(440))
                $scopeXY(a, a, { yRange: [10, 0] })
                a.out()
            `),
        ).toThrow(/yRange/);
    });

    test('non-finite range throws', () => {
        expect(() =>
            execPatch(`
                const a = $sine($hz(440))
                $scopeXY(a, a, { yRange: [0, Infinity] })
                a.out()
            `),
        ).toThrow(/yRange/);
    });
});
