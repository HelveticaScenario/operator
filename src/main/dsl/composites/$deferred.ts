import type { GraphBuilder } from '../graph';
import { DeferredCollection, DeferredModuleOutput } from '../graph';

/**
 * Create a DeferredCollection with placeholder signals that can be assigned later.
 * Useful for feedback loops and forward references.
 */
export function create$deferred(builder: GraphBuilder) {
    return (channels: number = 1): DeferredCollection => {
        if (channels < 1 || channels > 16) {
            throw new Error(
                `deferred() channels must be between 1 and 16, got ${channels}`,
            );
        }
        const items: DeferredModuleOutput[] = [];
        for (let i = 0; i < channels; i++) {
            items.push(new DeferredModuleOutput(builder));
        }
        return new DeferredCollection(...items);
    };
}
