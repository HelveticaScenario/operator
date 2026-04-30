import type { Collection, Signal } from '../graph';
import type { NamespaceTree } from '../factory';

type FactoryFn = (...args: unknown[]) => unknown;

/**
 * `$cross(count, playhead, range?, interpolationType?)` — crossfade across `count`
 * weights using a `$track` keyframe interpolator.
 *
 * @example
 * // crossfade 3 voices by a slow LFO
 * const osc = $sine(['c', 'e', 'g']);
 * const weights = $cross(osc.length, $sine('0.25hz').range(0, 1));
 * osc.amp(weights).out();
 */
export function create$cross(namespaceTree: NamespaceTree) {
    const $track = namespaceTree['$track'];
    if (typeof $track !== 'function') {
        throw new Error(
            'DSL execution error: "$cross" requires "$track" module',
        );
    }
    const track = $track as FactoryFn;

    return (
        count: number,
        playhead: Signal,
        range: [number, number] = [0, 5],
        interpolationType?: string,
    ): Collection => {
        const frames: [Signal[], number][] = [];
        for (let i = 0; i < count; i++) {
            const frame: Signal[] = Array.from(
                { length: count },
                () => range[0],
            );
            frame[i] = range[1];
            frames.push([frame, count > 1 ? i / (count - 1) : 0]);
        }
        return track(frames, { playhead, interpolationType }) as Collection;
    };
}
