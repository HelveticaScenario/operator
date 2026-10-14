import type { PatchGraph } from '@modular/core';
import {
    MAX_VIDEO_TAPS,
    type CompiledVideoShader,
} from '../shared/video/videoGraph';

/** The engine module that publishes a signal to a tap slot. */
const TAP_MODULE = '_videoTap';

type TapParams = { input: unknown; slot: number };

/** A tap's source signal as a comparable string, with module ids passed through `rename`. */
function signalKey(input: unknown, rename: (id: string) => string): string {
    return JSON.stringify(input, (key: string, value: unknown) =>
        key === 'module' && typeof value === 'string' ? rename(value) : value,
    );
}

/** The tap modules of `patch`. */
const tapsOf = (patch: PatchGraph) =>
    patch.modules.filter((m) => m.moduleType === TAP_MODULE);

/**
 * Keeps each audio signal a patch reads into video on the same engine tap
 * slot from one patch update to the next, so its samples carry on unbroken
 * through the update and the renderer keeps what it already holds of them.
 * The DSL numbers taps afresh for every patch; this renumbers them before the
 * patch reaches the engine, matching them against the taps of the patch the
 * engine is playing.
 */
export class VideoTapSlots {
    /** Each placed patch's stamp per slot, which changes whenever the slot gets another signal. */
    private readonly stamps = new WeakMap<PatchGraph, number[]>();
    private nextStamp = 1;

    /**
     * Moves the taps of `patch` onto the slots their signals have in
     * `playing`, the patch the engine is playing, and new signals onto free
     * slots, and renumbers `shader` to match. `moduleIdRemap` maps each module
     * id of `playing` that the engine carries over to its id in `patch`.
     * Returns the renumbered shader, which carries each slot's stamp.
     */
    assign(
        patch: PatchGraph,
        shader: CompiledVideoShader | null,
        playing: PatchGraph | null,
        moduleIdRemap: Record<string, string>,
    ): CompiledVideoShader | null {
        const rename = (id: string) => moduleIdRemap[id] ?? id;
        const keep = (id: string) => id;
        const playingStamps = (playing && this.stamps.get(playing)) ?? [];
        const previous = new Map<string, number>();
        const occupied = new Set<number>();
        for (const tap of playing ? tapsOf(playing) : []) {
            const { input, slot } = tap.params as TapParams;
            previous.set(signalKey(input, rename), slot);
            occupied.add(slot);
        }

        const taps = tapsOf(patch);
        const slotOf = new Map<number, number>();
        const taken = new Set<number>();
        const stamps: number[] = [];
        const unplaced: typeof taps = [];
        for (const tap of taps) {
            const { input, slot: dslSlot } = tap.params as TapParams;
            const slot = previous.get(signalKey(input, keep));
            if (
                slot === undefined ||
                taken.has(slot) ||
                playingStamps[slot] === undefined
            ) {
                unplaced.push(tap);
                continue;
            }
            slotOf.set(dslSlot, slot);
            taken.add(slot);
            stamps[slot] = playingStamps[slot];
        }
        // A new signal takes a slot no signal of the playing patch has where
        // one is free, so its slot's old samples are never mistaken for it.
        const free = Array.from({ length: MAX_VIDEO_TAPS }, (_, s) => s)
            .filter((s) => !taken.has(s))
            .sort(
                (a, b) =>
                    Number(occupied.has(a)) - Number(occupied.has(b)) || a - b,
            );
        for (const tap of unplaced) {
            const slot = free.shift()!;
            slotOf.set((tap.params as TapParams).slot, slot);
            stamps[slot] = this.nextStamp++;
        }

        for (const tap of taps) {
            const params = tap.params as TapParams;
            tap.params = { ...params, slot: slotOf.get(params.slot)! };
        }
        this.stamps.set(patch, stamps);
        if (shader === null) return null;
        const moved = (tap: number) => slotOf.get(tap) ?? tap;
        return {
            ...shader,
            histories: shader.histories.map((h) => ({
                ...h,
                tap: moved(h.tap),
            })),
            tapStamps: Array.from(
                { length: MAX_VIDEO_TAPS },
                (_, s) => stamps[s] ?? 0,
            ),
            uniforms: shader.uniforms.map((u) =>
                u.kind === 'tap' ? { ...u, tap: moved(u.tap) } : u,
            ),
        };
    }
}
