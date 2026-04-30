import type { ModuleSchema, ModuleState, PatchGraph, ScopeMode } from '@modular/core';
import type { ProcessedModuleSchema } from '../paramsSchema';
import { processSchemas } from '../paramsSchema';
import type { Bus } from './bus';
import type { DeferredModuleOutput } from './deferredOutput';
import type { ModuleOutput } from './moduleOutput';
import type {
    FactoryFunction,
    MonoOutGroup,
    MonoOutOptions,
    OutGroup,
    ScopeWithLocation,
    Signal,
    SourceLocation,
    StereoOutGroup,
    StereoOutOptions,
} from './types';
import { Collection } from './collection';
import { CollectionWithRange } from './collectionWithRange';
import { ModuleNode } from './moduleNode';
import { buildChannelCollections } from './buildOutputs';
import {
    buildDeferredStringMap,
    serializeModules,
    serializeScopes,
} from './serialize';

/**
 * GraphBuilder manages the construction of a PatchGraph from DSL code.
 * Tracks modules, generates deterministic IDs, and builds the final graph.
 */
export class GraphBuilder {
    private modules = new Map<string, ModuleState>();
    private counters = new Map<string, number>();
    private schemas: ProcessedModuleSchema[] = [];
    private schemaByName = new Map<string, ProcessedModuleSchema>();
    private scopes: ScopeWithLocation[] = [];
    /** Output groups keyed by baseChannel */
    private outGroups = new Map<number, OutGroup[]>();
    private factoryRegistry = new Map<string, FactoryFunction>();
    private sourceLocationMap = new Map<string, SourceLocation>();
    /** Track all deferred outputs for string replacement during toPatch */
    private deferredOutputs = new Map<string, DeferredModuleOutput>();
    /** Global tempo for ROOT_CLOCK in BPM (default: 120) */
    private tempo: number = 120;
    /** Whether $setTempo was explicitly called in the DSL */
    private tempoExplicitlySet: boolean = false;
    /** Global output gain signal (default: 2.5) */
    private outputGain: Signal = 2.5;
    /** Time signature numerator (beats per bar) for ROOT_CLOCK */
    private timeSignatureNumerator: number = 4;
    /** Time signature denominator (beat value) for ROOT_CLOCK */
    private timeSignatureDenominator: number = 4;
    private busses: Bus[] = [];
    private endOfChainCb: (
        mixed: Collection,
    ) => ModuleOutput | Collection | CollectionWithRange = (e) => e;
    private processingBusses: boolean = false;
    private processingEndOfChain: boolean = false;

    constructor(schemas: ModuleSchema[]) {
        this.schemas = processSchemas(schemas);
        this.schemaByName = new Map(this.schemas.map((s) => [s.name, s]));
    }

    /** Generate a deterministic ID for a module type */
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

    /** Add or update a module in the graph */
    addModule(
        moduleType: string,
        explicitId?: string,
        sourceLocation?: { line: number; column: number },
    ): ModuleNode {
        const id = this.generateId(moduleType, explicitId);

        if (this.modules.has(id)) {
            throw new Error(`Duplicate module id: ${id}`);
        }

        const schema = this.schemaByName.get(moduleType);
        if (!schema) {
            throw new Error(`Unknown module type: ${moduleType}`);
        }

        if (sourceLocation) {
            this.sourceLocationMap.set(id, {
                column: sourceLocation.column,
                idIsExplicit: Boolean(explicitId),
                line: sourceLocation.line,
            });
        }

        const moduleState: ModuleState = {
            id,
            idIsExplicit: Boolean(explicitId),
            moduleType,
            params: {},
        };

        this.modules.set(id, moduleState);
        return new ModuleNode(this, id, moduleType, schema);
    }

    getModule(id: string): ModuleState | undefined {
        return this.modules.get(id);
    }

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

    /** Set the global tempo for ROOT_CLOCK */
    setTempo(tempo: number): void {
        this.tempo = tempo;
        this.tempoExplicitlySet = true;
    }

    /** Set the global output gain (2.5 is default, 5.0 is unity) */
    setOutputGain(gain: Signal): void {
        this.outputGain = gain;
    }

    /** Set the time signature for ROOT_CLOCK */
    setTimeSignature(numerator: number, denominator: number): void {
        this.timeSignatureNumerator = numerator;
        this.timeSignatureDenominator = denominator;
    }

    setEndOfChainCb(
        cb: (
            mixed: Collection,
        ) => ModuleOutput | Collection | CollectionWithRange,
    ): void {
        if (this.processingEndOfChain) {
            throw new Error(
                '`$setEndOfChainCb` is not allowed in its own callback.',
            );
        }
        this.endOfChainCb = cb;
    }

    /** Get a factory function by module type name. Throws if not registered. */
    getFactory(moduleType: string): FactoryFunction {
        const factory = this.factoryRegistry.get(moduleType);
        if (!factory) {
            throw new Error(`Factory ${moduleType} not found`);
        }
        return factory;
    }

    /**
     * Build the final PatchGraph.
     *
     * Note: Uses factory functions for signal/mix modules for consistency,
     * which adds overhead from channel count derivation on every patch build.
     */
    toPatch(): PatchGraph {
        this.processingBusses = true;
        for (const bus of this.busses) {
            bus.lock();
        }
        for (const bus of this.busses) {
            // Bus callbacks register themselves
            bus.finalize();
        }

        const signalFactory = this.getFactory('$signal');
        const mixFactory = this.getFactory('$mix');

        if (this.outGroups.size > 0) {
            const channelCollections = buildChannelCollections(this.outGroups, {
                stereoMixer: this.getFactory('$stereoMix'),
                scaleAndShift: this.getFactory('$scaleAndShift'),
                curve: this.getFactory('$curve'),
            });
            const finalMix = mixFactory(channelCollections) as Collection;

            // Apply end-of-chain processing and global output gain
            this.processingEndOfChain = true;
            const gainedMix = this.endOfChainCb(finalMix).gain(this.outputGain);

            signalFactory(gainedMix, { id: 'ROOT_OUTPUT' });
        } else {
            // No outputs registered - silent root signal (0V)
            signalFactory(0, { id: 'ROOT_OUTPUT' });
        }

        // Update ROOT_CLOCK tempo with the current tempo setting
        const rootClock = this.modules.get('ROOT_CLOCK');
        if (rootClock) {
            rootClock.params.tempo = this.tempo;
            rootClock.params.numerator = this.timeSignatureNumerator;
            rootClock.params.denominator = this.timeSignatureDenominator;
            rootClock.params.tempoSet = this.tempoExplicitlySet;
        }

        const deferredStringMap = buildDeferredStringMap(this.deferredOutputs);

        const ret: PatchGraph = {
            modules: serializeModules(
                this.modules,
                this.deferredOutputs,
                deferredStringMap,
            ),
            scopes: serializeScopes(this.scopes, this.deferredOutputs),
        };

        console.log('Built PatchGraph:', ret);
        return ret;
    }

    /** Reset the builder state */
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

    /** Register module output(s) for stereo output routing */
    addOut(
        value: ModuleOutput | ModuleOutput[],
        options: StereoOutOptions = {},
    ): void {
        if (this.processingEndOfChain) {
            throw new Error(
                '`.out` is not allowed in the end of chain processor callback.',
            );
        }

        const baseChannel = options.baseChannel ?? 0;
        if (baseChannel < 0 || baseChannel > 14) {
            throw new Error(`baseChannel must be 0-14, got ${baseChannel}`);
        }

        const outputs = Array.isArray(value) ? [...value] : [value];
        const group: StereoOutGroup = {
            gain: options.gain,
            outputs,
            pan: options.pan,
            type: 'stereo',
            width: options.width,
        };

        const existing = this.outGroups.get(baseChannel) ?? [];
        existing.push(group);
        this.outGroups.set(baseChannel, existing);
    }

    /** Register module output(s) for mono output routing */
    addOutMono(
        value: ModuleOutput | ModuleOutput[],
        options: MonoOutOptions = {},
    ): void {
        if (this.processingEndOfChain) {
            throw new Error(
                '`.outMono` is not allowed in the end of chain processor callback.',
            );
        }

        const channel = options.channel ?? 0;
        if (channel < 0 || channel > 15) {
            throw new Error(`channel must be 0-15, got ${channel}`);
        }

        const outputs = Array.isArray(value) ? [...value] : [value];
        const group: MonoOutGroup = {
            gain: options.gain,
            outputs,
            type: 'mono',
        };

        const existing = this.outGroups.get(channel) ?? [];
        existing.push(group);
        this.outGroups.set(channel, existing);
    }

    addBus(bus: Bus) {
        if (this.processingEndOfChain) {
            throw new Error(
                '`$bus` is not allowed in the end of chain processor callback.',
            );
        } else if (this.processingBusses) {
            throw new Error('`$bus` is not allowed in other $bus callbacks');
        }
        this.busses.push(bus);
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
            channel: o.channel,
            moduleId: o.moduleId,
            portName: o.portName,
        }));

        this.scopes.push({
            channels,
            msPerFrame,
            range,
            sourceLocation,
            triggerThreshold: thresh,
        });
    }
}
