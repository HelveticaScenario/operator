/**
 * Tests for the $g1/$g2/$g3 signal-group taggers and the cartesian expansion
 * that resolves them into plain signal arrays before the PatchGraph is built.
 */

import { describe, expect, test } from 'vitest';
import type { PatchGraph } from '@modular/core';
import schemas from '@modular/core/schemas.json';
import type { ParamDescriptor, ProcessedModuleSchema } from '../paramsSchema';
import { executePatchScript } from '../executor';
import {
    $g1,
    $g2,
    $g3,
    assertSignalGroupsTopLevelOnly,
    expandSignalGroups,
    isSignalGroupLike,
} from '../signalGroups';

function execPatch(source: string): PatchGraph {
    return executePatchScript(source, schemas, {
        sampleRate: 48_000,
        workspaceRoot: '/workspace',
    }).patch;
}

function findModule(patch: PatchGraph, moduleType: string) {
    const found = patch.modules.filter((m) => m.moduleType === moduleType);
    expect(found.length).toBeGreaterThan(0);
    return found[0];
}

/** Minimal schema stub: every named param is a polyphonic signal input. */
function polySchema(
    names: string[],
    extra: Partial<ParamDescriptor>[] = [],
): ProcessedModuleSchema {
    const params: ParamDescriptor[] = [
        ...names.map((name) => ({
            name,
            kind: 'polySignal' as const,
            optional: true,
            isPolySignalInput: true,
        })),
        ...extra.map((d) => ({ kind: 'unknown', optional: true, ...d })),
    ] as ParamDescriptor[];
    const paramsByName: Record<string, ParamDescriptor> = {};
    for (const p of params) {
        paramsByName[p.name] = p;
    }
    return { params, paramsByName } as ProcessedModuleSchema;
}

describe('$g1/$g2/$g3 taggers', () => {
    test('produce tagged wrappers', () => {
        expect($g1([1, 2])).toEqual({
            __kind: 'SignalGroup',
            group: 1,
            signals: [1, 2],
        });
        expect($g2(5).group).toBe(2);
        expect($g3('c3').group).toBe(3);
        expect(isSignalGroupLike($g1(0))).toBe(true);
        expect(isSignalGroupLike([1, 2])).toBe(false);
        expect(isSignalGroupLike({ __kind: 'ParsedPattern' })).toBe(false);
    });

    test('reject nesting', () => {
        expect(() => $g2($g1([1]))).toThrow('cannot wrap another signal group');
    });
});

describe('assertSignalGroupsTopLevelOnly', () => {
    test('allows a wrapper at the root and plain values anywhere', () => {
        expect(() => assertSignalGroupsTopLevelOnly($g1([1, 2]))).not.toThrow();
        expect(() => assertSignalGroupsTopLevelOnly([1, 2, 3])).not.toThrow();
        expect(() =>
            assertSignalGroupsTopLevelOnly({ nested: { deep: [1] } }),
        ).not.toThrow();
    });

    test('rejects a wrapper inside an array', () => {
        expect(() => assertSignalGroupsTopLevelOnly([$g1([1]), 2])).toThrow(
            'must wrap the entire value',
        );
    });

    test('rejects a wrapper inside an object', () => {
        expect(() => assertSignalGroupsTopLevelOnly({ a: $g1([1]) })).toThrow(
            'must wrap the entire value',
        );
    });

    test('rejects a wrapper smuggled inside a wrapped array', () => {
        const outer = {
            __kind: 'SignalGroup',
            group: 1,
            signals: [$g2([1]), 2],
        };
        expect(() => assertSignalGroupsTopLevelOnly(outer)).toThrow(
            'must wrap the entire value',
        );
    });
});

describe('expandSignalGroups', () => {
    test('two groups: exact cartesian ordering, lower group fastest', () => {
        const schema = polySchema(['a', 'b']);
        const out = expandSignalGroups(
            { a: $g1(['A0', 'A1', 'A2']), b: $g2(['B0', 'B1']) },
            schema,
            '$test',
        );
        expect(out.a).toEqual(['A0', 'A1', 'A2', 'A0', 'A1', 'A2']);
        expect(out.b).toEqual(['B0', 'B0', 'B0', 'B1', 'B1', 'B1']);
    });

    test('mixed widths within one group cycle inside the group slots', () => {
        const schema = polySchema(['a', 'b', 'c']);
        const out = expandSignalGroups(
            {
                a: $g1(['A0', 'A1', 'A2']),
                b: $g1(['B0', 'B1']),
                c: $g2(['C0', 'C1']),
            },
            schema,
            '$test',
        );
        // W = [-,3,2,-]; stride g1=1, g2=3; total 6.
        expect(out.a).toEqual(['A0', 'A1', 'A2', 'A0', 'A1', 'A2']);
        // The narrower member follows its group slot ((ch % 3) % 2),
        // not plain channel cycling (ch % 2).
        expect(out.b).toEqual(['B0', 'B1', 'B0', 'B0', 'B1', 'B0']);
        expect(out.c).toEqual(['C0', 'C0', 'C0', 'C1', 'C1', 'C1']);
    });

    test('untagged params join group 0 and expand with the product', () => {
        const schema = polySchema(['c', 'd']);
        const out = expandSignalGroups(
            { c: ['C0', 'C1'], d: $g1(['D0', 'D1', 'D2']) },
            schema,
            '$test',
        );
        // W = [2,3,-,-]; stride g0=1, g1=2; total 6.
        expect(out.c).toEqual(['C0', 'C1', 'C0', 'C1', 'C0', 'C1']);
        expect(out.d).toEqual(['D0', 'D0', 'D1', 'D1', 'D2', 'D2']);
    });

    test('three groups multiply', () => {
        const schema = polySchema(['a', 'b', 'c']);
        const out = expandSignalGroups(
            { a: $g1([0, 1]), b: $g2([10, 11]), c: $g3([20, 21]) },
            schema,
            '$test',
        );
        expect(out.a).toEqual([0, 1, 0, 1, 0, 1, 0, 1]);
        expect(out.b).toEqual([10, 10, 11, 11, 10, 10, 11, 11]);
        expect(out.c).toEqual([20, 20, 20, 20, 21, 21, 21, 21]);
    });

    test('scalars stay scalar; wrapped scalars and 1-length arrays unwrap', () => {
        const schema = polySchema(['a', 'b', 'c', 'd']);
        const out = expandSignalGroups(
            { a: $g1([0, 1]), b: 5, c: $g2('c3'), d: $g3([7]) },
            schema,
            '$test',
        );
        expect(out.a).toEqual([0, 1]);
        expect(out.b).toBe(5);
        expect(out.c).toBe('c3');
        expect(out.d).toEqual([7]);
    });

    test('no wrappers: returns the same object reference', () => {
        const schema = polySchema(['a', 'b']);
        const params = { a: [1, 2, 3], b: 0 };
        expect(expandSignalGroups(params, schema, '$test')).toBe(params);
    });

    test('non-signal params are left alone and never expanded', () => {
        const schema = polySchema(
            ['a'],
            [{ name: 'mode', kind: 'string' }, { name: 'steps' }],
        );
        const steps = [1, 2, 3, 4];
        const out = expandSignalGroups(
            { a: $g1([0, 1]), mode: 'wrap', steps },
            schema,
            '$test',
        );
        expect(out.mode).toBe('wrap');
        expect(out.steps).toBe(steps);
    });

    test('rejects a wrapper on a mono (summing) input', () => {
        const schema = polySchema(
            ['input'],
            [
                {
                    name: 'width',
                    kind: 'polySignal',
                    isMonoSignalInput: true,
                },
            ],
        );
        expect(() =>
            expandSignalGroups(
                { input: 0, width: $g1([1, 2]) },
                schema,
                '$stereoMix',
            ),
        ).toThrow(
            '$g1: parameter "width" of $stereoMix is a mono (summing) input — signal groups are not supported here',
        );
    });

    test('rejects a wrapper on a non-signal param', () => {
        const schema = polySchema(['a'], [{ name: 'mode', kind: 'string' }]);
        expect(() =>
            expandSignalGroups({ mode: $g2('wrap') }, schema, '$test'),
        ).toThrow(
            '$g2: parameter "mode" of $test is not a polyphonic signal input — signal groups are not supported here',
        );
    });

    test('rejects a product over 64 channels, naming the group widths', () => {
        const schema = polySchema(['a', 'b']);
        expect(() =>
            expandSignalGroups(
                {
                    a: $g1(Array.from({ length: 16 }, (_, i) => i)),
                    b: $g2(Array.from({ length: 8 }, (_, i) => i)),
                },
                schema,
                '$sine',
            ),
        ).toThrow(
            'signal groups on $sine multiply to 128 channels (g1:16 x g2:8); the limit is 64',
        );
    });

    test('a full 64-channel product is allowed', () => {
        const schema = polySchema(['a', 'b']);
        const out = expandSignalGroups(
            {
                a: $g1(Array.from({ length: 8 }, (_, i) => i)),
                b: $g2(Array.from({ length: 8 }, (_, i) => i)),
            },
            schema,
            '$test',
        );
        expect(out.a).toHaveLength(64);
        expect(out.b).toHaveLength(64);
    });

    test('empty-array members are skipped, not expanded', () => {
        const schema = polySchema(['a', 'b']);
        const out = expandSignalGroups(
            { a: $g1([0, 1]), b: [] },
            schema,
            '$test',
        );
        expect(out.a).toEqual([0, 1]);
        expect(out.b).toEqual([]);
    });

    test('does not mutate the input params object', () => {
        const schema = polySchema(['a']);
        const wrapper = $g1([0, 1]);
        const params = { a: wrapper };
        const out = expandSignalGroups(params, schema, '$test');
        expect(params.a).toBe(wrapper);
        expect(out).not.toBe(params);
    });
});

describe('signal groups through the executor pipeline', () => {
    test('two-group $sine emits full cartesian arrays', () => {
        const patch = execPatch(`
            $sine($g1(["c3", "e3", "g3"]), {
                phaseOffset: $g2([0, 0.25]),
            }).out()
        `);
        const sine = findModule(patch, '$sine');
        expect(sine.params.freq).toEqual(['c3', 'e3', 'g3', 'c3', 'e3', 'g3']);
        expect(sine.params.phaseOffset).toEqual([0, 0, 0, 0.25, 0.25, 0.25]);
    });

    test('grouped module returns a full-product Collection', () => {
        const patch = execPatch(`
            const v = $sine($g1(["c3", "e3", "g3"]), {
                phaseOffset: $g2([0, 0.25]),
            });
            $mix([...v]).out()
        `);
        const sineId = findModule(patch, '$sine').id;
        const mix = findModule(patch, '$mix');
        const inputs = mix.params.inputs as Array<{
            type: string;
            module: string;
            channel: number;
        }>;
        expect(inputs).toHaveLength(6);
        inputs.forEach((cable, i) => {
            expect(cable.type).toBe('cable');
            expect(cable.module).toBe(sineId);
            expect(cable.channel).toBe(i);
        });
    });

    test('module outputs inside a group become cables before expansion', () => {
        const patch = execPatch(`
            $saw($g1([...$sine("1hz"), ...$sine("2hz")]), {
                phaseOffset: $g2([0, 0.5]),
            }).out()
        `);
        const saw = findModule(patch, '$saw');
        const freq = saw.params.freq as Array<{ type: string; module: string }>;
        expect(freq).toHaveLength(4);
        expect(freq.every((s) => s.type === 'cable')).toBe(true);
        // Group 1 cycles fastest: sine1, sine2, sine1, sine2.
        expect(freq[0].module).toBe(freq[2].module);
        expect(freq[1].module).toBe(freq[3].module);
        expect(freq[0].module).not.toBe(freq[1].module);
        expect(saw.params.phaseOffset).toEqual([0, 0, 0.5, 0.5]);
    });

    test('positional-arg groups work', () => {
        const patch = execPatch(`$sine($g1(["c3", "e3"])).out()`);
        expect(findModule(patch, '$sine').params.freq).toEqual(['c3', 'e3']);
    });

    test('ungrouped modules are untouched', () => {
        const patch = execPatch(`$sine(["c3", "e3", "g3"]).out()`);
        expect(findModule(patch, '$sine').params.freq).toEqual([
            'c3',
            'e3',
            'g3',
        ]);
    });

    test('nested groups throw', () => {
        expect(() => execPatch(`$sine($g1($g2(["c3"]))).out()`)).toThrow(
            'cannot wrap another signal group',
        );
    });

    test('a group inside an array param throws', () => {
        expect(() => execPatch(`$sine([$g1("c3"), "e3"]).out()`)).toThrow(
            'must wrap the entire value passed to a parameter',
        );
    });

    test('a group on a mono (summing) input throws', () => {
        expect(() =>
            execPatch(`$stereoMix($saw("c3"), { width: $g1([1, 2]) }).out()`),
        ).toThrow('is a mono (summing) input');
    });

    test('a group on a non-signal param throws', () => {
        expect(() =>
            execPatch(`$sine("c3", { fmMode: $g1("linear") }).out()`),
        ).toThrow('is not a polyphonic signal input');
    });

    test('a product over 64 channels throws with group widths', () => {
        expect(() =>
            execPatch(`
                const wide = (n) => Array.from({ length: n }, (_, i) => i * 0.01);
                $sine($g1(wide(16)), {
                    phaseOffset: $g2(wide(8)),
                }).out()
            `),
        ).toThrow('multiply to 128 channels (g1:16 x g2:8); the limit is 64');
    });
});
