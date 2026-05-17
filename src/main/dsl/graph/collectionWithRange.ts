import { BaseCollection, Collection } from './collection';
import { ModuleOutputWithRange } from './moduleOutputWithRange';
import type { PolySignal } from './types';

/**
 * Collection of ModuleOutputWithRange instances.
 * Use .range(outMin, outMax) to remap using stored min/max values.
 */
export class CollectionWithRange extends BaseCollection<ModuleOutputWithRange> {
    /** Remap outputs from their known range to a new output range */
    range(outMin: PolySignal, outMax: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$remap');
        if (!factory) {
            throw new Error('Factory for util.remap not registered');
        }
        return factory(
            this.items,
            outMin,
            outMax,
            this.items.map((o) => o.minValue),
            this.items.map((o) => o.maxValue),
        ) as Collection;
    }
}

/** Create a CollectionWithRange from ModuleOutputWithRange instances */
export const $r = (
    ...args: (ModuleOutputWithRange | Iterable<ModuleOutputWithRange>)[]
): CollectionWithRange =>
    new CollectionWithRange(
        ...args.flatMap((arg) =>
            arg instanceof ModuleOutputWithRange ? [arg] : [...arg],
        ),
    );
