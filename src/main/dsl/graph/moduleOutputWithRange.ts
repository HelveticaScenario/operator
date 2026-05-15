import type { GraphBuilder } from './builder';
import type { Collection } from './collection';
import type { PolySignal } from './types';
import { ModuleOutput } from './moduleOutput';

/**
 * ModuleOutputWithRange extends ModuleOutput with known output range metadata.
 * Provides .range() method to easily remap the output to a new range.
 */
export class ModuleOutputWithRange extends ModuleOutput {
    readonly minValue: number;
    readonly maxValue: number;

    constructor(
        builder: GraphBuilder,
        moduleId: string,
        portName: string,
        channel: number = 0,
        minValue: number,
        maxValue: number,
    ) {
        super(builder, moduleId, portName, channel);
        this.minValue = minValue;
        this.maxValue = maxValue;
    }

    /**
     * Remap this output from its known range to a new range.
     * Creates a remap module internally.
     */
    range(outMin: PolySignal, outMax: PolySignal): Collection {
        const factory = this.builder.getFactory('$remap');
        return factory(
            this,
            outMin,
            outMax,
            this.minValue,
            this.maxValue,
        ) as Collection;
    }
}
