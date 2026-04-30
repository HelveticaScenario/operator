import { BaseCollection } from './collection';
import { ModuleOutput } from './moduleOutput';
import type { DeferredModuleOutput } from './deferredOutput';

/**
 * DeferredCollection is a collection of DeferredModuleOutput instances.
 * Provides a .set() method to assign ModuleOutputs to all contained deferred outputs.
 */
export class DeferredCollection extends BaseCollection<DeferredModuleOutput> {
    /**
     * Set the values for all deferred outputs in this collection.
     * @param outputs - A ModuleOutput or iterable of ModuleOutputs to distribute across outputs
     */
    set(outputs: ModuleOutput | Iterable<ModuleOutput>): void {
        if (outputs instanceof ModuleOutput) {
            outputs = [outputs];
        }

        const outputsArr = Array.from(outputs);

        // Distribute signals across deferred outputs
        for (let i = 0; i < this.items.length; i++) {
            this.items[i].set(outputsArr[i % outputsArr.length]);
        }
    }
}
