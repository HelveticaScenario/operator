/**
 * `.erratic()` builds a hidden `_erratic` module. On a ranged signal it passes
 * the range as `min`/`max`, clamps by default, and the result keeps the range;
 * on an unranged signal it passes neither and never clamps.
 */

import { beforeEach, describe, expect, test } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { DSLContext } from '../factories';
import {
    Collection,
    CollectionWithRange,
    type GraphBuilder,
    ModuleOutputWithRange,
} from '../GraphBuilder';
import { executePatchScript } from '../executor';

let builder: GraphBuilder;

beforeEach(() => {
    builder = new DSLContext(schemas).getBuilder();
});

function erraticParams(): Record<string, unknown> {
    const node = builder
        .toPatch()
        .modules.find((m) => m.moduleType === '_erratic');
    expect(node).toBeDefined();
    return node!.params as Record<string, unknown>;
}

describe('.erratic()', () => {
    test('ranged output passes its range, clamps, and keeps the range', () => {
        const out = (builder.getFactory('$sine')('1hz') as CollectionWithRange)
            .range(0, 10)[0]
            .erratic(2, { speed: 3 });

        expect(out).toBeInstanceOf(CollectionWithRange);
        expect(out[0].minValue).toBe(0);
        expect(out[0].maxValue).toBe(10);
        const params = erraticParams();
        expect(params).toMatchObject({ min: 0, max: 10, clamp: true });
    });

    test('clamp: false on a ranged collection', () => {
        const out = (builder.getFactory('$sine')('1hz') as CollectionWithRange)
            .range(-3, -1)
            .erratic(5, { clamp: false });

        expect(out).toBeInstanceOf(CollectionWithRange);
        expect(erraticParams()).toMatchObject({
            min: [-3],
            max: [-1],
            clamp: false,
        });
    });

    test('per-output ranges of a ranged collection are kept', () => {
        const sine = builder.getFactory('$sine');
        const ranged = new CollectionWithRange(
            ...(sine('1hz') as CollectionWithRange).range(0, 1),
            ...(sine('2hz') as CollectionWithRange).range(2, 5),
        );
        const out = ranged.erratic(1);

        expect(out.items.map((o) => [o.minValue, o.maxValue])).toEqual([
            [0, 1],
            [2, 5],
        ]);
    });

    test('unranged signal stays unranged and passes no range or clamp', () => {
        const out = builder.$c('c3').erratic(1, {
            bias: 2,
            clamp: true,
        } as object);

        expect(out).toBeInstanceOf(Collection);
        expect(out[0]).not.toBeInstanceOf(ModuleOutputWithRange);
        const params = erraticParams();
        expect(params.bias).toBe(2);
        expect(params.min).toBeUndefined();
        expect(params.max).toBeUndefined();
        expect(params.clamp).toBeUndefined();
    });

    test('mode and sparse pass through on a ranged signal', () => {
        (builder.getFactory('$sine')('1hz') as CollectionWithRange)
            .range(0, 5)
            .erratic(5, { mode: 'blend', sparse: true });
        expect(erraticParams()).toMatchObject({
            mode: 'blend',
            sparse: true,
            clamp: true,
        });
    });

    test('sparse passes through on an unranged signal', () => {
        builder.$c('c3').erratic(1, { sparse: true });
        const params = erraticParams();
        expect(params.sparse).toBe(true);
        expect(params.mode).toBeUndefined();
    });

    test('_erratic is not a global or a .$ method', () => {
        const run = (src: string) =>
            executePatchScript(src, schemas, {
                sampleRate: 48_000,
                workspaceRoot: '/workspace',
            });
        expect(() => run('$erratic(0, 1).out()')).toThrow();
        expect(() => run('$sine(0).$.erratic(1).out()')).toThrow();
    });
});
