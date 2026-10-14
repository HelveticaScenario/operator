import type { PatchGraph } from '@modular/core';
import {
    MAX_VIDEO_TAPS,
    type CompiledVideoShader,
} from '../shared/video/videoGraph';

/** The engine module that publishes a signal to a tap slot. */
const TAP_MODULE = '_videoTap';

/** A tap's source signal as a comparable string, with module ids passed through `rename`. */
function signalKey(input: unknown, rename: (id: string) => string): string {
    return JSON.stringify(input, (key: string, value: unknown) =>
        key === 'module' && typeof value === 'string' ? rename(value) : value,
    );
}

/**
 * Keeps each audio signal a patch reads into video on the same engine tap
 * slot from one patch update to the next, so its samples carry on unbroken
 * through the update and the renderer keeps what it already holds of them.
 * The DSL numbers taps afresh for every patch; this renumbers them before the
 * patch reaches the engine.
 */
export class VideoTapSlots {
    /** The source of the signal on each slot, by slot, in the latest patch. */
    private sources: unknown[] = [];
    /** Each slot's signal stamp, which changes whenever the slot gets another signal. */
    private stamps: number[] = [];
    private nextStamp = 1;

    /**
     * Moves the taps of `patch` onto the slots their signals had in the
     * previous patch, and new signals onto free slots, and renumbers `shader`
     * to match. `moduleIdRemap` maps each module id of the previous patch that
     * the engine carries over to its id in this one. Returns the renumbered
     * shader, which carries each slot's stamp.
     */
    assign(
        patch: PatchGraph,
        shader: CompiledVideoShader | null,
        moduleIdRemap: Record<string, string>,
    ): CompiledVideoShader | null {
        const taps = patch.modules.filter((m) => m.moduleType === TAP_MODULE);
        const rename = (id: string) => moduleIdRemap[id] ?? id;
        const keep = (id: string) => id;
        const previous = new Map<string, number>();
        this.sources.forEach((source, slot) => {
            if (source !== undefined) {
                previous.set(signalKey(source, rename), slot);
            }
        });

        const slotOf = new Map<number, number>();
        const taken = new Set<number>();
        const unplaced: typeof taps = [];
        for (const tap of taps) {
            const params = tap.params as { input: unknown; slot: number };
            const slot = previous.get(signalKey(params.input, keep));
            if (slot === undefined || taken.has(slot)) {
                unplaced.push(tap);
                continue;
            }
            slotOf.set(params.slot, slot);
            taken.add(slot);
        }
        // A new signal takes a slot no signal of the previous patch had where
        // one is free, so its slot's old samples are never mistaken for it.
        const free = Array.from({ length: MAX_VIDEO_TAPS }, (_, s) => s)
            .filter((s) => !taken.has(s))
            .sort(
                (a, b) =>
                    Number(this.sources[a] !== undefined) -
                        Number(this.sources[b] !== undefined) || a - b,
            );
        for (const tap of unplaced) {
            const params = tap.params as { slot: number };
            const slot = free.shift()!;
            slotOf.set(params.slot, slot);
            taken.add(slot);
            this.stamps[slot] = this.nextStamp++;
        }

        const sources: unknown[] = [];
        for (const tap of taps) {
            const params = tap.params as { input: unknown; slot: number };
            const slot = slotOf.get(params.slot)!;
            sources[slot] = params.input;
            tap.params = { ...params, slot };
        }
        this.sources = sources;
        if (shader === null) return null;
        const moved = (tap: number) => slotOf.get(tap) ?? tap;
        return {
            ...shader,
            histories: shader.histories.map((h) => ({
                ...h,
                tap: moved(h.tap),
            })),
            tapStamps: Array.from({ length: MAX_VIDEO_TAPS }, (_, s) =>
                sources[s] === undefined ? 0 : this.stamps[s],
            ),
            uniforms: shader.uniforms.map((u) =>
                u.kind === 'tap' ? { ...u, tap: moved(u.tap) } : u,
            ),
        };
    }
}
