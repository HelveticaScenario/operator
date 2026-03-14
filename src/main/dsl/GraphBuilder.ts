import type {
    ModuleSchema,
    ModuleState,
    PatchGraph,
    Scope,
    ScopeMode,
} from '@modular/core';
import type { ProcessedModuleSchema } from './paramsSchema';
import { processSchemas } from './paramsSchema';
import { captureSourceLocation } from './captureSourceLocation';

import z from 'zod';

export const PORT_MAX_CHANNELS = 16;

/** Exponent used by .gain() for perceptual amplitude curve */
const GAIN_CURVE_EXP = 3;

/**
 * Scope with an optional source location captured at call time.
 * The base Scope type comes from Rust (napi); the extra field is
 * ignored by Rust but flows through to the renderer via IPC.
 */
export type ScopeWithLocation = Scope & {
    sourceLocation?: { line: number; column: number };
};

// Extended OutputSchema interface that includes optional range
export interface OutputSchemaWithRange {
    name: string;
    description: string;
    polyphonic?: boolean;
    minValue?: number;
    maxValue?: number;
}

const ResolvedModuleOutput = z.object({
    type: z.literal('cable'),
    module: z.string(),
    port: z.string(),
    channel: z.number().optional(),
});

export type ResolvedModuleOutput = z.infer<typeof ResolvedModuleOutput>;

// Type definitions for Collection system
export type OrArray<T> = T | T[];
export type Signal = number | string | ModuleOutput;
export type PolySignal = OrArray<Signal> | Iterable<ModuleOutput>;

// ─── Cartesian product helpers ────────────────────────────────────────────────

export type ElementsOf<T extends unknown[][]> = {
    [K in keyof T]: T[K] extends (infer E)[] ? E : never;
};

/**
 * Compute the Cartesian product of the given arrays.
 *
 * Returns every possible combination of one element from each array.
 * Pairs well with the array overload of `.pipe()` to fan a signal across
 * multiple parameter dimensions.
 *
 * @param arrays - Zero or more arrays to combine
 * @returns Array of tuples, one per combination
 *
 * @example $cartesian([1, 2], ['a', 'b'])
 * // → [[1,'a'], [1,'b'], [2,'a'], [2,'b']]
 */
export function $cartesian<A extends unknown[][]>(
    ...arrays: A
): ElementsOf<A>[] {
    return arrays.reduce<unknown[][]>(
        (acc, arr) => acc.flatMap((combo) => arr.map((val) => [...combo, val])),
        [[]],
    ) as ElementsOf<A>[];
}

/** Options for stereo output routing */
export interface StereoOutOptions {
    /** Base output channel (0-14, default 0). Left plays on baseChannel, right on baseChannel+1 */
    baseChannel?: number;
    /** Output gain. If set, a scaleAndShift module is added after the stereo mix */
    gain?: PolySignal;
    /** Pan position (-5 = left, 0 = center, +5 = right). Default 0 */
    pan?: PolySignal;
    /** Stereo width/spread (0 = no spread, 5 = full spread). Default 0 */
    width?: Signal;
}

/** Options for mono output routing */
export interface MonoOutOptions {
    /** Output channel (0-15, default 0) */
    channel?: number;
    /** Output gain. If set, a scaleAndShift module is added after the mix */
    gain?: PolySignal;
}

/** Internal storage for a stereo output group */
export interface StereoOutGroup {
    type: 'stereo';
    outputs: ModuleOutput[];
    gain?: PolySignal;
    pan?: PolySignal;
    width?: PolySignal;
}

/** Internal storage for a mono output group */
export interface MonoOutGroup {
    type: 'mono';
    outputs: ModuleOutput[];
    gain?: PolySignal;
}

export type OutGroup = StereoOutGroup | MonoOutGroup;

/**
 * BaseCollection provides iterable, indexable container for ModuleOutput arrays
 * with chainable DSP methods (amplitude, shift, scope, out).
 */
export class BaseCollection<T extends ModuleOutput> implements Iterable<T> {
    [index: number]: T;
    readonly items: T[] = [];

    constructor(...args: T[]) {
        this.items.push(...args);
        for (const [i, arg] of args.entries()) {
            this[i] = arg;
        }
    }

    get length(): number {
        return this.items.length;
    }

    [Symbol.iterator](): Iterator<T> {
        return this.items.values();
    }

    /**
     * Scale all outputs by a factor
     */
    amplitude(factor: PolySignal): Collection {
        if (this.items.length === 0) return new Collection();
        const factory = this.items[0].builder.getFactory('$scaleAndShift');
        if (!factory) {
            throw new Error('Factory for util.scaleAndShift not registered');
        }
        return factory(this.items, factor) as Collection;
    }

    /** Alias for {@link amplitude} */
    amp(factor: PolySignal): Collection {
        return this.amplitude(factor);
    }

    /**
     * Shift all outputs by an offset
     */
    shift(offset: PolySignal): Collection {
        if (this.items.length === 0) return new Collection();
        const factory = this.items[0].builder.getFactory('$scaleAndShift');
        if (!factory) {
            throw new Error('Factory for util.scaleAndShift not registered');
        }
        return factory(this.items, undefined, offset) as Collection;
    }

    /**
     * Scale all outputs by a factor with a perceptual (audio taper) curve.
     * Chains $curve → $scaleAndShift with exponent 3.
     */
    gain(level: PolySignal): Collection {
        if (this.items.length === 0) return new Collection();
        const curveFactory = this.items[0].builder.getFactory('$curve');
        const scaleFactory = this.items[0].builder.getFactory('$scaleAndShift');
        if (!curveFactory || !scaleFactory) {
            throw new Error(
                'Factory for $curve or $scaleAndShift not registered',
            );
        }
        const curvedLevel = curveFactory(level, GAIN_CURVE_EXP);
        return scaleFactory(this.items, curvedLevel) as Collection;
    }

    /**
     * Apply a power curve to all outputs. Creates a $curve module internally.
     */
    exp(factor: PolySignal = GAIN_CURVE_EXP): Collection {
        if (this.items.length === 0) return new Collection();
        const factory = this.items[0].builder.getFactory('$curve');
        if (!factory) {
            throw new Error('Factory for $curve not registered');
        }
        return factory(this.items, factor) as Collection;
    }

    /**
     * Add scope visualization for all outputs in the collection
     */
    scope(config?: {
        msPerFrame?: number;
        triggerThreshold?: number;
        triggerWaitToRender?: boolean;
        range?: [number, number];
    }): this {
        if (this.items.length > 0) {
            const loc = captureSourceLocation();
            this.items[0].builder.addScope(this.items, config, loc);
        }
        return this;
    }

    /**
     * Send all outputs to speakers as stereo
     * @param baseChannel - Base output channel (0-15, default 0)
     * @param options.gain - Output gain
     * @param options.pan - Pan position (-5 = left, 0 = center, +5 = right)
     * @param options.width - Stereo width/spread (0 = no spread, 5 = full spread)
     */
    out(baseChannel: number = 0, options: StereoOutOptions = {}): this {
        if (this.items.length > 0) {
            this.items[0].builder.addOut([...this.items], {
                ...options,
                baseChannel,
            });
        }
        return this;
    }

    /**
     * Send all outputs to speakers as mono
     * @param channel - Output channel (0-15, default 0)
     * @param gain - Output gain
     */
    outMono(channel: number = 0, gain?: PolySignal): this {
        if (this.items.length > 0) {
            this.items[0].builder.addOutMono([...this.items], {
                channel,
                gain,
            });
        }
        return this;
    }

    pipe<T>(pipelineFunc: (self: this) => T): T;
    pipe<T extends ModuleOutput | Iterable<ModuleOutput>, E>(
        pipelineFunc: (self: this, item: E) => T,
        array: E[],
    ): Collection;
    pipe<T>(
        pipelineFunc: (self: this, ...args: unknown[]) => T,
        ...arrays: unknown[][]
    ): T | Collection {
        if (arrays.length === 0) return pipelineFunc(this);
        return $c(
            ...arrays[0].map(
                (item) =>
                    pipelineFunc(this, item) as
                        | ModuleOutput
                        | Iterable<ModuleOutput>,
            ),
        );
    }

    pipeMix(
        pipelineFunc: (
            self: this,
        ) => ModuleOutput | BaseCollection<ModuleOutput>,
        mix: Value & PolySignal = 2.5,
    ): Collection {
        const clampFactory = this.items[0].builder.getFactory('$clamp');
        if (!clampFactory) {
            throw new Error('Factory for $clamp not registered');
        }
        const remapFactory = this.items[0].builder.getFactory('$remap');
        if (!remapFactory) {
            throw new Error('Factory for $remap not registered');
        }
        const mixFactory = this.items[0].builder.getFactory('$mix');
        if (!mixFactory) {
            throw new Error('Factory for $mix not registered');
        }
        const result = pipelineFunc(this);
        // Remap mix from 0-5 to 5-0 for crossfade between original and transformed signals
        return mixFactory([
            this.amplitude(
                clampFactory(remapFactory(mix, 5, 0, 0, 5), { min: 0, max: 5 }),
            ),
            result.amplitude(
                clampFactory(mix, { min: 0, max: 5 }) as Value & PolySignal,
            ),
        ]) as Collection;
    }

    toString(): string {
        return `[${this.items.map((item) => item.toString()).join(',')}]`;
    }
}

/**
 * Collection of ModuleOutput instances.
 * Use .range(outMin, outMax, inMin, inMax) to remap with explicit input range.
 */
export class Collection extends BaseCollection<ModuleOutput> {
    constructor(...args: ModuleOutput[]) {
        super(...args);
    }

    /**
     * Remap outputs from explicit input range to output range
     */
    range(
        outMin: PolySignal,
        outMax: PolySignal,
        inMin: PolySignal,
        inMax: PolySignal,
    ): Collection {
        if (this.items.length === 0) return new Collection();
        const factory = this.items[0].builder.getFactory('$remap');
        if (!factory) {
            throw new Error('Factory for util.remap not registered');
        }
        return factory(this.items, outMin, outMax, inMin, inMax) as Collection;
    }
}

/**
 * Collection of ModuleOutputWithRange instances.
 * Use .range(outMin, outMax) to remap using stored min/max values.
 */
export class CollectionWithRange extends BaseCollection<ModuleOutputWithRange> {
    constructor(...args: ModuleOutputWithRange[]) {
        super(...args);
    }

    /**
     * Remap outputs from their known range to a new output range
     */
    range(outMin: PolySignal, outMax: PolySignal): Collection {
        if (this.items.length === 0) return new Collection();
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

/**
 * Create a Collection from ModuleOutput instances
 */
export const $c = (
    ...args: (ModuleOutput | Iterable<ModuleOutput>)[]
): Collection =>
    new Collection(
        ...args.flatMap((arg) =>
            arg instanceof ModuleOutput ? [arg] : [...arg],
        ),
    );

/**
 * Create a CollectionWithRange from ModuleOutputWithRange instances
 */
export const $r = (
    ...args: (ModuleOutputWithRange | Iterable<ModuleOutputWithRange>)[]
): CollectionWithRange =>
    new CollectionWithRange(
        ...args.flatMap((arg) =>
            arg instanceof ModuleOutputWithRange ? [arg] : [...arg],
        ),
    );

/**
 * Factory function type for creating modules via DSL.
 * Returns the module's output(s) directly rather than the ModuleNode.
 */
export type FactoryFunction = (
    ...args: unknown[]
) => ModuleOutput | Collection | CollectionWithRange;

/**
 * Source location information for mapping validation errors back to DSL code.
 */
export interface SourceLocation {
    /** 1-based line number in the DSL source */
    line: number;
    /** 1-based column number in the DSL source */
    column: number;
    /** Whether the module ID was explicitly set by the user */
    idIsExplicit: boolean;
}

/**
 * GraphBuilder manages the construction of a PatchGraph from DSL code.
 * It tracks modules, generates deterministic IDs, and builds the final graph.
 *
 * Note: Factory functions add overhead from channel count derivation but provide
 * consistency across all module creation paths.
 */
export class GraphBuilder {
    private modules: Map<string, ModuleState> = new Map();
    private counters: Map<string, number> = new Map();
    private schemas: ProcessedModuleSchema[] = [];
    private schemaByName: Map<string, ProcessedModuleSchema> = new Map();
    private scopes: ScopeWithLocation[] = [];
    /** Output groups keyed by baseChannel */
    private outGroups: Map<number, OutGroup[]> = new Map();
    private factoryRegistry: Map<string, FactoryFunction> = new Map();
    private sourceLocationMap: Map<string, SourceLocation> = new Map();
    /** Track all deferred outputs for string replacement during toPatch */
    private deferredOutputs: Map<string, DeferredModuleOutput> = new Map();
    /** Global tempo for ROOT_CLOCK in BPM (default: 120) */
    private tempo: number = 120;
    /** Global output gain signal (default: 2.5) */
    private outputGain: Signal = 2.5;
    /** Time signature numerator (beats per bar) for ROOT_CLOCK */
    private timeSignatureNumerator: number = 4;
    /** Time signature denominator (beat value) for ROOT_CLOCK */
    private timeSignatureDenominator: number = 4;

    constructor(schemas: ModuleSchema[]) {
        this.schemas = processSchemas(schemas);
        this.schemaByName = new Map(this.schemas.map((s) => [s.name, s]));
    }

    /**
     * Generate a deterministic ID for a module type
     */
    private generateId(moduleType: string, explicitId?: string): string {
        if (explicitId) {
            return explicitId;
        }

        let counter = (this.counters.get(moduleType) || 0) + 1;
        let id = `${moduleType}-${counter}`;

        // If the generated ID is already taken (e.g. by an explicit ID),
        // keep incrementing until we find a free one.
        while (this.modules.has(id)) {
            counter++;
            id = `${moduleType}-${counter}`;
        }

        this.counters.set(moduleType, counter);
        return id;
    }

    /**
     * Add or update a module in the graph
     */
    addModule(
        moduleType: string,
        explicitId?: string,
        sourceLocation?: { line: number; column: number },
    ): ModuleNode {
        const id = this.generateId(moduleType, explicitId);

        if (this.modules.has(id)) {
            throw new Error(`Duplicate module id: ${id}`);
        }

        // Check if module type exists in schemas
        const schema = this.schemaByName.get(moduleType);
        if (!schema) {
            throw new Error(`Unknown module type: ${moduleType}`);
        }

        // Store source location for error mapping
        if (sourceLocation) {
            this.sourceLocationMap.set(id, {
                line: sourceLocation.line,
                column: sourceLocation.column,
                idIsExplicit: Boolean(explicitId),
            });
        }

        // Module params start empty; only user-provided values are set.
        const params: Record<string, unknown> = {};

        const moduleState: ModuleState = {
            id,
            moduleType,
            idIsExplicit: Boolean(explicitId),
            params,
        };

        this.modules.set(id, moduleState);
        return new ModuleNode(this, id, moduleType, schema);
    }

    /**
     * Get a module by ID
     */
    getModule(id: string): ModuleState | undefined {
        return this.modules.get(id);
    }

    /**
     * Set a parameter value for a module
     */
    setParam(moduleId: string, paramName: string, value: unknown): void {
        const module = this.modules.get(moduleId);
        if (!module) {
            throw new Error(`Module not found: ${moduleId}`);
        }
        module.params[paramName] = value;
    }

    /**
     * Register factory functions for late binding.
     * Called by DSLContext after factory creation to enable internal factory usage.
     */
    setFactoryRegistry(factories: Map<string, FactoryFunction>): void {
        this.factoryRegistry = factories;
    }

    /**
     * Set the global tempo for ROOT_CLOCK
     * @param tempo - Tempo in BPM (plain number)
     */
    setTempo(tempo: number): void {
        this.tempo = tempo;
    }

    /**
     * Set the global output gain
     * @param gain - Signal value for output gain (2.5 is default, 5.0 is unity)
     */
    setOutputGain(gain: Signal): void {
        this.outputGain = gain;
    }

    /**
     * Set the time signature for ROOT_CLOCK
     * @param numerator - Beats per bar (positive integer)
     * @param denominator - Beat value (positive integer)
     */
    setTimeSignature(numerator: number, denominator: number): void {
        this.timeSignatureNumerator = numerator;
        this.timeSignatureDenominator = denominator;
    }

    /**
     * Get a factory function by module type name.
     * Returns undefined if factories haven't been registered yet.
     */
    getFactory(moduleType: string): FactoryFunction | undefined {
        return this.factoryRegistry.get(moduleType);
    }

    /**
     * Build the final PatchGraph
     *
     * Note: Uses factory functions for signal/mix modules for consistency,
     * which adds overhead from channel count derivation on every patch build.
     */
    toPatch(): PatchGraph {
        const signalFactory = this.getFactory('$signal');
        const mixFactory = this.getFactory('$mix');
        const stereoMixerFactory = this.getFactory('$stereoMix');
        const scaleAndShiftFactory = this.getFactory('$scaleAndShift');
        const curveFactory = this.getFactory('$curve');

        if (
            !signalFactory ||
            !mixFactory ||
            !stereoMixerFactory ||
            !scaleAndShiftFactory ||
            !curveFactory
        ) {
            throw new Error(
                'Required factories (signal, mix, stereoMixer, util.scaleAndShift, curve) not registered',
            );
        }

        // Process output groups and build channel collections
        if (this.outGroups.size > 0) {
            // Collect all channel collections to mix together
            const allChannelCollections: (ModuleOutput | undefined)[][] = [];

            // Sort by baseChannel for deterministic processing
            const sortedChannels = [...this.outGroups.keys()].sort(
                (a, b) => a - b,
            );

            for (const baseChannel of sortedChannels) {
                const groups = this.outGroups.get(baseChannel)!;

                for (const group of groups) {
                    let outputSignals: ModuleOutput[];

                    if (group.type === 'stereo') {
                        // Create stereoMixer with the outputs
                        const stereoOut = stereoMixerFactory(group.outputs, {
                            pan: group.pan ?? 0,
                            width: group.width ?? 0,
                        }) as Collection;

                        // Apply gain if specified
                        if (group.gain !== undefined) {
                            const curvedAmp = curveFactory(
                                group.gain,
                                GAIN_CURVE_EXP,
                            );
                            const gained = scaleAndShiftFactory(
                                [...stereoOut],
                                curvedAmp,
                            ) as Collection;
                            outputSignals = [...gained];
                        } else {
                            outputSignals = [...stereoOut];
                        }
                    } else {
                        // Mono: use mix module
                        const mixOut = (
                            stereoMixerFactory(group.outputs, {
                                pan: -5,
                                width: 0,
                            }) as Collection
                        )[0];

                        // Apply gain if specified
                        let finalOut: ModuleOutput;
                        if (group.gain !== undefined) {
                            const curvedAmp = curveFactory(
                                group.gain,
                                GAIN_CURVE_EXP,
                            );
                            finalOut = scaleAndShiftFactory(
                                mixOut,
                                curvedAmp,
                            ) as ModuleOutput;
                        } else {
                            finalOut = mixOut;
                        }
                        outputSignals = [finalOut];
                    }

                    // Build channel collection with baseChannel silent channels prepended
                    const channelCollection: (ModuleOutput | undefined)[] = [];

                    // Add silent channels for baseChannel offset
                    for (let i = 0; i < baseChannel; i++) {
                        // Push 0 (Signal::Volts(0.0)) to represent silence
                        channelCollection.push(undefined);
                    }

                    // Add the actual output signals
                    channelCollection.push(...outputSignals);

                    allChannelCollections.push(channelCollection);
                }
            }
            // Mix all channel collections together using poly mix
            // Each collection contributes to corresponding output channels
            const finalMix = mixFactory(allChannelCollections) as Collection;

            // Apply global output gain
            const gainedMix = finalMix.gain(this.outputGain);

            // Create root signal module with the final mix
            signalFactory(gainedMix, { id: 'ROOT_OUTPUT' });
        } else {
            // No outputs registered - create silent root signal (0V)
            signalFactory(0, { id: 'ROOT_OUTPUT' });
        }

        // Update ROOT_CLOCK tempo with the current tempo setting
        const rootClock = this.modules.get('ROOT_CLOCK');
        if (rootClock) {
            rootClock.params.tempo = this.tempo;
            if (this.timeSignatureNumerator !== undefined) {
                rootClock.params.numerator = this.timeSignatureNumerator;
            }
            if (this.timeSignatureDenominator !== undefined) {
                rootClock.params.denominator = this.timeSignatureDenominator;
            }
        }

        // Build a map of deferred output strings to their resolved output strings
        const deferredStringMap = new Map<string, string | null>();
        for (const deferred of this.deferredOutputs.values()) {
            const deferredStr = deferred.toString();
            const resolved = deferred.resolve();
            if (resolved) {
                deferredStringMap.set(deferredStr, resolved.toString());
            } else {
                deferredStringMap.set(deferredStr, null);
            }
        }

        const ret = {
            modules: Array.from(this.modules.values()).map((m) => {
                // First replace signals (ModuleOutput -> cable objects)
                const replacedParams = replaceDeferred(
                    replaceSignals(m.params),
                    this.deferredOutputs,
                );
                // Then replace any deferred strings with resolved strings
                const finalParams = replaceDeferredStrings(
                    replacedParams,
                    deferredStringMap,
                );
                return {
                    ...m,
                    params: finalParams,
                };
            }),
            scopes: this.scopes
                .map((scope) => {
                    const resolvedChannels = scope.channels.map((ch) => {
                        const deferredOutput = this.deferredOutputs.get(
                            ch.moduleId,
                        );
                        if (deferredOutput) {
                            const resolved = deferredOutput.resolve();
                            if (resolved) {
                                return {
                                    moduleId: resolved.moduleId,
                                    portName: resolved.portName,
                                    channel: ch.channel,
                                };
                            }
                            return null;
                        }
                        return ch;
                    });
                    // If any channel couldn't be resolved, skip the scope
                    if (resolvedChannels.some((ch) => ch === null)) {
                        return null;
                    }
                    return {
                        ...scope,
                        channels: resolvedChannels,
                    } as ScopeWithLocation;
                })
                .filter(
                    (s: ScopeWithLocation | null): s is ScopeWithLocation =>
                        s !== null,
                ),
        };

        console.log('Built PatchGraph:', ret);
        return ret;
    }

    /**
     * Reset the builder state
     */
    reset(): void {
        this.modules.clear();
        this.scopes = [];
        this.counters.clear();
        this.outGroups.clear();
        this.sourceLocationMap.clear();
        this.deferredOutputs.clear();
        this.tempo = 120;
        this.outputGain = 2.5;
        this.timeSignatureNumerator = 4;
        this.timeSignatureDenominator = 4;
    }

    /**
     * Get the source location map for error reporting.
     * Maps module IDs to their source locations in the DSL code.
     */
    getSourceLocationMap(): Map<string, SourceLocation> {
        return this.sourceLocationMap;
    }

    /**
     * Register module output(s) for stereo output routing
     */
    addOut(
        value: ModuleOutput | ModuleOutput[],
        options: StereoOutOptions = {},
    ): void {
        const baseChannel = options.baseChannel ?? 0;
        if (baseChannel < 0 || baseChannel > 14) {
            throw new Error(`baseChannel must be 0-14, got ${baseChannel}`);
        }

        const outputs = Array.isArray(value) ? [...value] : [value];
        const group: StereoOutGroup = {
            type: 'stereo',
            outputs,
            gain: options.gain,
            pan: options.pan,
            width: options.width,
        };

        const existing = this.outGroups.get(baseChannel) ?? [];
        existing.push(group);
        this.outGroups.set(baseChannel, existing);
    }

    /**
     * Register module output(s) for mono output routing
     */
    addOutMono(
        value: ModuleOutput | ModuleOutput[],
        options: MonoOutOptions = {},
    ): void {
        const channel = options.channel ?? 0;
        if (channel < 0 || channel > 15) {
            throw new Error(`channel must be 0-15, got ${channel}`);
        }

        const outputs = Array.isArray(value) ? [...value] : [value];
        const group: MonoOutGroup = {
            type: 'mono',
            outputs,
            gain: options.gain,
        };

        const existing = this.outGroups.get(channel) ?? [];
        existing.push(group);
        this.outGroups.set(channel, existing);
    }

    /**
     * Register a deferred output for tracking.
     * Called by DeferredModuleOutput constructor.
     */
    registerDeferred(deferred: DeferredModuleOutput): void {
        this.deferredOutputs.set(deferred.moduleId, deferred);
    }

    addScope(
        value: ModuleOutput | ModuleOutput[],
        config: {
            msPerFrame?: number;
            triggerThreshold?: number;
            triggerWaitToRender?: boolean;
            range?: [number, number];
        } = {},
        sourceLocation?: { line: number; column: number },
    ) {
        const { msPerFrame = 500, triggerThreshold, range = [-5, 5] } = config;
        const realTriggerThreshold: number | undefined =
            triggerThreshold !== undefined
                ? triggerThreshold * 1000
                : undefined;
        const triggerWaitToRender = config.triggerWaitToRender ?? true;
        let thresh: [number, ScopeMode] | undefined = undefined;
        if (realTriggerThreshold !== undefined) {
            thresh = [
                realTriggerThreshold,
                triggerWaitToRender ? 'Wait' : 'Roll',
            ];
        }

        const outputs = Array.isArray(value) ? value : [value];
        const channels = outputs.map((o) => ({
            moduleId: o.moduleId,
            portName: o.portName,
            channel: o.channel,
        }));

        this.scopes.push({
            channels,
            msPerFrame,
            triggerThreshold: thresh,
            range,
            sourceLocation,
        });
    }
}

type Value = number | ModuleOutput | ModuleOutput[] | ModuleNode | ModuleNode[];

/**
 * ModuleNode represents a module instance in the DSL (internal use only)
 * Users interact with ModuleOutput directly, not ModuleNode
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

    /**
     * Set the channel count derived from Rust-side analysis.
     */
    _setDerivedChannelCount(channels: number): void {
        this._channelCount = channels;
    }

    /**
     * Get an output port of this module
     */
    _output(
        portName: string,
        polyphonic: boolean = false,
    ): ModuleOutput | Collection | ModuleOutputWithRange | CollectionWithRange {
        // Verify output exists
        const outputSchema = this.schema.outputs.find(
            (o) => o.name === portName,
        ) as OutputSchemaWithRange | undefined;
        if (!outputSchema) {
            throw new Error(
                `Module ${this.moduleType} does not have output: ${portName}`,
            );
        }

        // Check if this output has range metadata
        const hasRange =
            outputSchema.minValue !== undefined &&
            outputSchema.maxValue !== undefined;

        if (polyphonic) {
            // Return Collection(WithRange) for each channel (based on derived channel count)
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
            } else {
                const outputs: ModuleOutput[] = [];
                for (let i = 0; i < this.channelCount; i++) {
                    outputs.push(
                        new ModuleOutput(this.builder, this.id, portName, i),
                    );
                }
                return new Collection(...outputs);
            }
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

/**
 * ModuleOutput represents an output port that can be connected or transformed
 */
export class ModuleOutput {
    readonly builder: GraphBuilder;
    readonly moduleId: string;
    readonly portName: string;
    readonly channel: number = 0;

    constructor(
        builder: GraphBuilder,
        moduleId: string,
        portName: string,
        channel: number = 0,
    ) {
        this.builder = builder;
        this.moduleId = moduleId;
        this.portName = portName;
        this.channel = channel;
    }

    /**
     * Scale this output by a factor
     */
    amplitude(factor: Value): Collection {
        const factory = this.builder.getFactory('$scaleAndShift');
        if (!factory) {
            throw new Error('Factory for util.scaleAndShift not registered');
        }
        return factory(this, factor) as Collection;
    }

    /** Alias for {@link amplitude} */
    amp(factor: Value): Collection {
        return this.amplitude(factor);
    }

    /**
     * Shift this output by an offset
     */
    shift(offset: Value): Collection {
        const factory = this.builder.getFactory('$scaleAndShift');
        if (!factory) {
            throw new Error('Factory for util.scaleAndShift not registered');
        }
        return factory(this, undefined, offset) as Collection;
    }

    /**
     * Scale this output by a factor with a perceptual (audio taper) curve.
     * Chains $curve → $scaleAndShift with exponent 3.
     */
    gain(level: Value): Collection {
        const curveFactory = this.builder.getFactory('$curve');
        const scaleFactory = this.builder.getFactory('$scaleAndShift');
        if (!curveFactory || !scaleFactory) {
            throw new Error(
                'Factory for $curve or $scaleAndShift not registered',
            );
        }
        const curvedLevel = curveFactory(level, GAIN_CURVE_EXP);
        return scaleFactory(this, curvedLevel) as Collection;
    }

    /**
     * Apply a power curve to this output. Creates a $curve module internally.
     */
    exp(factor: Value = GAIN_CURVE_EXP): Collection {
        const factory = this.builder.getFactory('$curve');
        if (!factory) {
            throw new Error('Factory for $curve not registered');
        }
        return factory(this, factor) as Collection;
    }

    scope(config?: {
        msPerFrame?: number;
        triggerThreshold?: number;
        triggerWaitToRender?: boolean;
        range?: [number, number];
    }): this {
        const loc = captureSourceLocation();
        this.builder.addScope(this, config, loc);
        return this;
    }

    /**
     * Send this output to speakers as stereo
     * @param baseChannel - Base output channel (0-15, default 0)
     * @param options.gain - Output gain (adds util.scaleAndShift after stereo mix)
     * @param options.pan - Pan position (-5 = left, 0 = center, +5 = right)
     * @param options.width - Stereo width/spread (0 = no spread, 5 = full spread)
     */
    out(baseChannel: number = 0, options: StereoOutOptions = {}): this {
        this.builder.addOut(this, { ...options, baseChannel });
        return this;
    }

    /**
     * Send this output to speakers as mono
     * @param channel - Output channel (0-15, default 0)
     * @param gain - Output gain
     */
    outMono(channel: number = 0, gain?: PolySignal): this {
        this.builder.addOutMono(this, { channel, gain });
        return this;
    }

    pipe<T>(pipelineFunc: (self: this) => T): T;
    pipe<T extends ModuleOutput | Iterable<ModuleOutput>, E>(
        pipelineFunc: (self: this, item: E) => T,
        array: E[],
    ): Collection;
    pipe<T>(
        pipelineFunc: (self: this, ...args: unknown[]) => T,
        ...arrays: unknown[][]
    ): T | Collection {
        if (arrays.length === 0) return pipelineFunc(this);
        return $c(
            ...arrays[0].map(
                (item) =>
                    pipelineFunc(this, item) as
                        | ModuleOutput
                        | Iterable<ModuleOutput>,
            ),
        );
    }

    pipeMix(
        pipelineFunc: (
            self: this,
        ) => ModuleOutput | BaseCollection<ModuleOutput>,
        options?: Record<string, unknown>,
    ): Collection {
        const mixFactory = this.builder.getFactory('$mix');
        if (!mixFactory) {
            throw new Error('Factory for $mix not registered');
        }
        const result = pipelineFunc(this);
        return mixFactory([this, result], options) as Collection;
    }

    /**
     * Remap this output from explicit input range to output range
     */
    range(
        outMin: PolySignal,
        outMax: PolySignal,
        inMin: PolySignal,
        inMax: PolySignal,
    ): Collection {
        const factory = this.builder.getFactory('$remap');
        if (!factory) {
            throw new Error('Factory for $remap not registered');
        }
        return factory(this, outMin, outMax, inMin, inMax) as Collection;
    }

    toString(): string {
        return `module(${this.moduleId}:${this.portName}:${this.channel})`;
    }
}

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
        if (!factory) {
            throw new Error('Factory for remap not registered');
        }
        return factory(
            this,
            outMin,
            outMax,
            this.minValue,
            this.maxValue,
        ) as Collection;
    }
}

/** Type for transforms that return a new ModuleOutput */
type OutputTransform = (output: ModuleOutput) => ModuleOutput;
/** Type for side effects that operate on a ModuleOutput but don't return a new one */
type OutputSideEffect = (output: ModuleOutput) => void;

/**
 * DeferredModuleOutput is a placeholder for a signal that will be assigned later.
 * Useful for feedback loops and forward references in the DSL.
 * Supports the same chainable methods as ModuleOutput (amplitude, shift, scope, out, outMono).
 * Transforms are stored and applied when the deferred signal is resolved.
 */
export class DeferredModuleOutput extends ModuleOutput {
    private resolvedModuleOutput: ModuleOutput | null = null;
    private resolving: boolean = false;
    static idCounter = 0;

    constructor(builder: GraphBuilder) {
        super(
            builder,
            `DEFERRED-${DeferredModuleOutput.idCounter++}`,
            'output',
        );
        // Register this deferred output with the builder for string replacement during toPatch
        builder.registerDeferred(this);
    }

    /**
     * Set the actual signal this deferred output should resolve to.
     * @param signal - The signal to resolve to (number, string, or ModuleOutput)
     */
    set(signal: ModuleOutput): void {
        this.resolvedModuleOutput = signal;
    }

    /**
     * Resolve this deferred output to an actual ModuleOutput.
     * @returns The resolved ModuleOutput, or null if not set.
     */
    resolve(): ModuleOutput | null {
        if (this.resolving) {
            throw new Error(
                'Circular reference detected while resolving DeferredModuleOutput',
            );
        }

        if (this.resolvedModuleOutput === null) {
            return null;
        }

        let output = this.resolvedModuleOutput;
        if (output instanceof DeferredModuleOutput) {
            this.resolving = true;
            let resolved = output.resolve();
            this.resolving = false;

            if (resolved === null) {
                return null;
            }
            output = resolved;
        }

        return output;
    }
}

/**
 * DeferredCollection is a collection of DeferredModuleOutput instances.
 * Provides a .set() method to assign ModuleOutputs to all contained deferred outputs.
 */
export class DeferredCollection extends BaseCollection<DeferredModuleOutput> {
    constructor(...args: DeferredModuleOutput[]) {
        super(...args);
    }

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

type Replacer = (key: string, value: unknown) => unknown;

export function replaceValues(input: unknown, replacer: Replacer): unknown {
    function walk(key: string, value: unknown): unknown {
        const replaced = replacer(key, value);

        // Match JSON.stringify behavior
        if (replaced === undefined) {
            return undefined;
        }

        if (typeof replaced !== 'object' || replaced === null) {
            return replaced;
        }

        if (Array.isArray(replaced)) {
            return replaced
                .map((v, i) => walk(String(i), v))
                .filter((v) => v !== undefined);
        }

        const out: Record<string, unknown> = {};
        for (const [key, value] of Object.entries(replaced)) {
            const v = walk(key, value);
            if (v !== undefined) {
                out[key] = v;
            }
        }
        return out;
    }

    // JSON.stringify starts with key ""
    return walk('', input);
}

export function replaceSignals(input: unknown): unknown {
    return replaceValues(input, (_key, value) => {
        // Replace Collection instances with their items array
        if (value instanceof BaseCollection) {
            return [...value];
        }

        return valueToSignal(value);
    });
}

/**
 * Recursively replace deferred output strings with resolved output strings in params.
 * This handles cases where a DeferredModuleOutput was stringified (e.g., in pattern strings).
 */
export function replaceDeferredStrings(
    input: unknown,
    deferredStringMap: Map<string, string | null>,
): unknown {
    if (typeof input === 'string') {
        // Replace all occurrences of deferred strings with resolved strings
        let result = input;
        for (const [deferredStr, resolvedStr] of deferredStringMap) {
            const splitResult = result.split(deferredStr);
            if (splitResult.length > 1) {
                if (resolvedStr === null) {
                    throw new Error(
                        `Unset DeferredModuleOutput used in string: "${input}"`,
                    );
                }

                result = splitResult.join(resolvedStr);
            }
        }
        return result;
    }

    if (Array.isArray(input)) {
        return input.map((item) =>
            replaceDeferredStrings(item, deferredStringMap),
        );
    }

    if (typeof input === 'object' && input !== null) {
        const result: Record<string, unknown> = {};
        for (const [key, value] of Object.entries(input)) {
            result[key] = replaceDeferredStrings(value, deferredStringMap);
        }
        return result;
    }

    return input;
}

function replaceDeferred(
    input: unknown,
    deferredOutputs: Map<string, DeferredModuleOutput>,
): unknown {
    function replace(value: unknown): unknown {
        const maybeResolvedModuleOutput = ResolvedModuleOutput.safeParse(value);
        if (maybeResolvedModuleOutput.success) {
            const resolved = deferredOutputs.get(
                maybeResolvedModuleOutput.data.module,
            );
            if (resolved) {
                return valueToSignal(resolved.resolve());
            } else {
                return maybeResolvedModuleOutput.data;
            }
        }
        return value;
    }
    return replaceValues(input, (_key, value) => {
        // Replace Collection instances with their items array
        if (value instanceof BaseCollection) {
            return [...value];
        }

        return replace(value);
    });
}

function valueToSignal(value: unknown): unknown {
    if (value instanceof ModuleOutput) {
        return {
            type: 'cable',
            module: value.moduleId,
            port: value.portName,
            channel: value.channel,
        };
    } else if (value === null || value === undefined) {
        // Silence: 0 becomes Signal::Volts(0.0) in Rust
        return 0;
    }
    // It's a number
    return value;
}
