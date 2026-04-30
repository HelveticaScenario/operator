import type { GraphBuilder } from './builder';
import type { ProcessedModuleSchema } from '../paramsSchema';
import type { OutputSchemaWithRange } from './types';
import { Collection } from './collection';
import { CollectionWithRange } from './collectionWithRange';
import { ModuleOutput } from './moduleOutput';
import { ModuleOutputWithRange } from './moduleOutputWithRange';
import { replaceSignals } from './signalResolution';

/**
 * ModuleNode represents a module instance in the DSL (internal use only).
 * Users interact with ModuleOutput directly, not ModuleNode.
 */
export class ModuleNode {
    readonly builder: GraphBuilder;
    readonly id: string;
    readonly moduleType: string;
    readonly schema: ProcessedModuleSchema;
    private _channelCount: number = 1;

    constructor(
        builder: GraphBuilder,
        id: string,
        moduleType: string,
        schema: ProcessedModuleSchema,
    ) {
        this.builder = builder;
        this.id = id;
        this.moduleType = moduleType;
        this.schema = schema;
    }

    /**
     * Get the number of channels this module produces.
     * Set by Rust-side derivation via _setDerivedChannelCount.
     */
    get channelCount(): number {
        return this._channelCount;
    }

    _setParam(paramName: string, value: unknown): this {
        this.builder.setParam(this.id, paramName, replaceSignals(value));
        return this;
    }

    /**
     * Get a snapshot of the current params for this module.
     * Used for Rust-side channel count derivation.
     */
    getParamsSnapshot(): Record<string, unknown> {
        return this.builder.getModule(this.id)?.params ?? {};
    }

    /** Set the channel count derived from Rust-side analysis. */
    _setDerivedChannelCount(channels: number): void {
        this._channelCount = channels;
    }

    /** Get an output port of this module */
    _output(
        portName: string,
        polyphonic: boolean = false,
    ): ModuleOutput | Collection | ModuleOutputWithRange | CollectionWithRange {
        const outputSchema = this.schema.outputs.find(
            (o) => o.name === portName,
        ) as OutputSchemaWithRange | undefined;
        if (!outputSchema) {
            throw new Error(
                `Module ${this.moduleType} does not have output: ${portName}`,
            );
        }

        const hasRange =
            outputSchema.minValue !== undefined &&
            outputSchema.maxValue !== undefined;

        if (polyphonic) {
            if (hasRange) {
                const outputs: ModuleOutputWithRange[] = [];
                for (let i = 0; i < this.channelCount; i++) {
                    outputs.push(
                        new ModuleOutputWithRange(
                            this.builder,
                            this.id,
                            portName,
                            i,
                            outputSchema.minValue!,
                            outputSchema.maxValue!,
                        ),
                    );
                }
                return new CollectionWithRange(...outputs);
            }
            const outputs: ModuleOutput[] = [];
            for (let i = 0; i < this.channelCount; i++) {
                outputs.push(
                    new ModuleOutput(this.builder, this.id, portName, i),
                );
            }
            return new Collection(...outputs);
        }

        if (hasRange) {
            return new ModuleOutputWithRange(
                this.builder,
                this.id,
                portName,
                0,
                outputSchema.minValue!,
                outputSchema.maxValue!,
            );
        }
        return new ModuleOutput(this.builder, this.id, portName);
    }
}
