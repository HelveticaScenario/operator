import { describe, expect, it } from 'vitest';
import type { PatchGraph } from '@modular/core';
import type { CompiledVideoShader } from '../../shared/video/videoGraph';
import { VideoTapSlots } from '../videoTapSlots';

/** A patch whose taps read `sources` (module ids) on DSL slots 0, 1, ... */
const patchOf = (...sources: string[]): PatchGraph =>
    ({
        modules: sources.map((module, slot) => ({
            id: `_videoTap-${slot}`,
            moduleType: '_videoTap',
            params: {
                input: { channel: 0, module, port: 'output', type: 'cable' },
                slot,
            },
        })),
        scopes: [],
        vuMeters: [],
    }) as unknown as PatchGraph;

/** A shader reading every DSL slot of `count` as history row and uniform. */
const shaderOf = (count: number): CompiledVideoShader =>
    ({
        histories: Array.from({ length: count }, (_, tap) => ({
            samples: 512,
            tap,
            trigger: false,
        })),
        uniforms: Array.from({ length: count }, (_, tap) => ({
            kind: 'tap',
            slot: tap,
            tap,
            value: 0,
        })),
    }) as unknown as CompiledVideoShader;

const slotsOf = (patch: PatchGraph) =>
    patch.modules.map((m) => (m.params as { slot: number }).slot);

describe('VideoTapSlots', () => {
    it('keeps a signal on its slot when the DSL numbers it differently', () => {
        const slots = new VideoTapSlots();
        const first = patchOf('a', 'b');
        slots.assign(first, shaderOf(2), {});
        const second = patchOf('b', 'a');
        const shader = slots.assign(second, shaderOf(2), {})!;
        expect(slotsOf(second)).toEqual(slotsOf(first).reverse());
        expect(shader.histories.map((h) => h.tap)).toEqual(slotsOf(second));
        expect(
            shader.uniforms.map((u) => (u.kind === 'tap' ? u.tap : -1)),
        ).toEqual(slotsOf(second));
    });

    it('stamps a carried slot the same and a new signal afresh', () => {
        const slots = new VideoTapSlots();
        const before = slots.assign(patchOf('a', 'b'), shaderOf(2), {})!;
        const second = patchOf('a', 'c');
        const after = slots.assign(second, shaderOf(2), {})!;
        const [a, c] = slotsOf(second);
        expect(after.tapStamps![a]).toBe(before.tapStamps![a]);
        expect(after.tapStamps![c]).not.toBe(0);
        expect(before.tapStamps).not.toContain(after.tapStamps![c]);
    });

    it('puts a new signal on a slot no signal of the previous patch had', () => {
        const slots = new VideoTapSlots();
        slots.assign(patchOf('a', 'b'), shaderOf(2), {});
        const second = patchOf('a', 'c');
        slots.assign(second, shaderOf(2), {});
        expect(slotsOf(second)[1]).toBe(2);
    });

    it('follows a module the engine carries over under a new id', () => {
        const slots = new VideoTapSlots();
        const before = slots.assign(patchOf('sine-1'), shaderOf(1), {})!;
        const second = patchOf('sine-2');
        const after = slots.assign(second, shaderOf(1), {
            'sine-1': 'sine-2',
        })!;
        expect(slotsOf(second)).toEqual([0]);
        expect(after.tapStamps![0]).toBe(before.tapStamps![0]);
    });

    it('marks the slots a shader does not use with stamp 0', () => {
        const slots = new VideoTapSlots();
        const shader = slots.assign(patchOf('a'), shaderOf(1), {})!;
        expect(shader.tapStamps![0]).not.toBe(0);
        expect(shader.tapStamps!.slice(1).every((s) => s === 0)).toBe(true);
    });

    it('places the same patch the same way when asked twice', () => {
        const slots = new VideoTapSlots();
        slots.assign(patchOf('a', 'b'), shaderOf(2), {});
        const once = patchOf('c', 'a');
        const first = slots.assign(once, shaderOf(2), {})!;
        const again = patchOf('c', 'a');
        const second = slots.assign(again, shaderOf(2), {})!;
        expect(slotsOf(again)).toEqual(slotsOf(once));
        expect(second.tapStamps).toEqual(first.tapStamps);
    });
});
