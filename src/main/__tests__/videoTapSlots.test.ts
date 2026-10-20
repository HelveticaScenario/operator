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
        slots.assign(first, shaderOf(2), null, {});
        const second = patchOf('b', 'a');
        const shader = slots.assign(second, shaderOf(2), first, {})!;
        expect(slotsOf(second)).toEqual(slotsOf(first).reverse());
        expect(shader.histories.map((h) => h.tap)).toEqual(slotsOf(second));
        expect(
            shader.uniforms.map((u) => (u.kind === 'tap' ? u.tap : -1)),
        ).toEqual(slotsOf(second));
    });

    it('stamps a carried slot the same and a new signal afresh', () => {
        const slots = new VideoTapSlots();
        const first = patchOf('a', 'b');
        const before = slots.assign(first, shaderOf(2), null, {})!;
        const second = patchOf('a', 'c');
        const after = slots.assign(second, shaderOf(2), first, {})!;
        const [a, c] = slotsOf(second);
        expect(after.tapStamps![a]).toBe(before.tapStamps![a]);
        expect(after.tapStamps![c]).not.toBe(0);
        expect(before.tapStamps).not.toContain(after.tapStamps![c]);
    });

    it('puts a new signal on a slot no signal of the playing patch has', () => {
        const slots = new VideoTapSlots();
        const first = patchOf('a', 'b');
        slots.assign(first, shaderOf(2), null, {});
        const second = patchOf('a', 'c');
        slots.assign(second, shaderOf(2), first, {});
        expect(slotsOf(second)[1]).toBe(2);
    });

    it('follows a module the engine carries over under a new id', () => {
        const slots = new VideoTapSlots();
        const first = patchOf('sine-1');
        const before = slots.assign(first, shaderOf(1), null, {})!;
        const second = patchOf('sine-2');
        const after = slots.assign(second, shaderOf(1), first, {
            'sine-1': 'sine-2',
        })!;
        expect(slotsOf(second)).toEqual([0]);
        expect(after.tapStamps![0]).toBe(before.tapStamps![0]);
    });

    it('marks the slots a shader does not use with stamp 0', () => {
        const slots = new VideoTapSlots();
        const shader = slots.assign(patchOf('a'), shaderOf(1), null, {})!;
        expect(shader.tapStamps![0]).not.toBe(0);
        expect(shader.tapStamps!.slice(1).every((s) => s === 0)).toBe(true);
    });

    it('matches against the playing patch, whatever was placed since', () => {
        const slots = new VideoTapSlots();
        const playing = patchOf('a', 'b');
        const before = slots.assign(playing, shaderOf(2), null, {})!;
        // A patch the engine rejects is placed but never plays.
        slots.assign(patchOf('c', 'd'), shaderOf(2), playing, {});
        const next = patchOf('b', 'a');
        const after = slots.assign(next, shaderOf(2), playing, {})!;
        expect(slotsOf(next)).toEqual([1, 0]);
        expect(after.tapStamps!.slice(0, 2)).toEqual(
            before.tapStamps!.slice(0, 2),
        );
    });

    it('gives every slot a new stamp when nothing is playing', () => {
        const slots = new VideoTapSlots();
        const first = patchOf('a');
        const before = slots.assign(first, shaderOf(1), null, {})!;
        const after = slots.assign(patchOf('a'), shaderOf(1), null, {})!;
        expect(after.tapStamps![0]).not.toBe(before.tapStamps![0]);
    });
});
