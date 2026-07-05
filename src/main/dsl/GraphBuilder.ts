import type {
    ModuleSchema,
    ModuleSpec,
    PatchGraph,
    Scope,
    ScopeChannel,
    ScopeMode,
    ScopeXy,
    ScopeXyPair,
} from '@modular/core';
import type { ProcessedModuleSchema } from './paramsSchema';
import {
    dollarMethodName,
    processSchemas,
    qualifiesForDollarChain,
} from './paramsSchema';
import { captureSourceLocation } from './captureSourceLocation';

import z from 'zod';

export const PORT_MAX_CHANNELS = 64;

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

/**
 * Per-call $scopeXY state with an optional source location. Lives on
 * GraphBuilder until `toPatch` resolves the deferred outputs.
 */
export type ScopeXYWithLocation = {
    pairs: ScopeXyPair[];
    xRange: [number, number];
    yRange: [number, number];
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
    channel: z.number().optional(),
    module: z.string(),
    port: z.string(),
    type: z.literal('cable'),
});

export type ResolvedModuleOutput = z.infer<typeof ResolvedModuleOutput>;

// Type definitions for Collection system
export type OrArray<T> = T | T[];
export type Signal = number | string | ModuleOutput;
export type PolySignal = OrArray<Signal> | Iterable<ModuleOutput>;

/** Structural type satisfied by every chainable output kind (output or collection). */
type Amplifiable = { amplitude(factor: PolySignal): Collection };

/**
 * Runtime shape of a `.$.`/`.$m.` proxy: every qualifying module is exposed as
 * a method, looked up lazily by name. User-facing per-module signatures come
 * from the generated `DollarChain`/`DollarMixChain` interfaces, not this type.
 */
type DollarChainProxy = Record<string, (...args: unknown[]) => unknown>;

/**
 * A node in the `.$.` namespace tree: leaf methods (`fold` → `$unstable.shape.fold`)
 * plus child sub-namespaces (`shape` → node), so dotted module names of any depth
 * (`$unstable.shape.fold` → `.$.unstable.shape.fold`) resolve by walking segments.
 */
interface DollarNamespaceNode {
    leaves: Map<string, string>;
    children: Map<string, DollarNamespaceNode>;
}

/**
 * A buffer output reference — returned by `$buffer()`, passed to readers
 * (like `$bufRead`, `$delayRead`) as their `buffer` param.
 */
export type BufferOutputRef = {
    type: 'buffer_ref';
    module: string;
    port: string;
    channels: number;
    frameCount: number;
};

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

interface SendGroup {
    outputs: ModuleOutput[];
    gain?: PolySignal;
}

export class Bus {
    private builder: GraphBuilder;
    private cb: (mixed: Collection) => unknown;
    private sendGroups: SendGroup[] = [];
    private locked: boolean = false;

    constructor(builder: GraphBuilder, cb: (mixed: Collection) => unknown) {
        this.builder = builder;
        this.cb = cb;

        builder.addBus(this);
    }

    addSend(value: ModuleOutput | ModuleOutput[], gain?: PolySignal): void {
        if (this.locked) {
            throw new Error('`.send` is not allowed in $bus callbacks');
        }
        const outputs = Array.isArray(value) ? [...value] : [value];
        const group: SendGroup = {
            gain,
            outputs,
        };
        this.sendGroups.push(group);
    }

    lock() {
        this.locked = true;
    }

    finalize() {
        const mixFactory = this.builder.getFactory('$mix');
        const mixed = mixFactory(
            this.sendGroups.map((e) => {
                const coll = this.builder.$c(e.outputs);
                if (e.gain !== undefined) {
                    return coll.gain(e.gain);
                }
                return coll;
            }),
        ) as Collection;
        this.cb(mixed);
    }
}

/**
 * Normalize a {@link PolySignal} into a flat array of scalar {@link Signal}s.
 * Scalars (number, string, ModuleOutput) wrap to a single-element array;
 * arrays and collections spread. The scalar cases are guarded before the
 * spread because strings are themselves iterable.
 */
function toSignalArray(v: PolySignal): Signal[] {
    if (
        typeof v === 'number' ||
        typeof v === 'string' ||
        v instanceof ModuleOutput
    ) {
        return [v];
    }
    return [...v];
}

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
     * Scale all outputs by a linear factor (5 = unity, 2.5 = half, 10 = 2x).
     *
     * For perceptual (audio-taper) volume control, use {@link gain} instead.
     */
    amplitude(factor: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
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
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$scaleAndShift');
        if (!factory) {
            throw new Error('Factory for util.scaleAndShift not registered');
        }
        return factory(this.items, undefined, offset) as Collection;
    }

    /**
     * Offset all pitches by an absolute frequency amount, in Hz.
     * Creates an $addHz module internally.
     */
    addHz(offset: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$addHz');
        if (!factory) {
            throw new Error('Factory for util.addHz not registered');
        }
        return factory(this.items, offset) as Collection;
    }

    /**
     * Multiply all pitches by a frequency factor (2 = octave up, 0.5 = down).
     * Creates a $mulHz module internally.
     */
    mulHz(factor: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$mulHz');
        if (!factory) {
            throw new Error('Factory for util.mulHz not registered');
        }
        return factory(this.items, factor) as Collection;
    }

    /**
     * Scale all outputs by a factor with a perceptual (audio taper) curve
     * (5 = unity, 0 = silence). Chains $curve → $scaleAndShift with exponent 3.
     *
     * For linear amplitude scaling, use {@link amplitude} instead.
     */
    gain(level: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
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
        if (this.items.length === 0) {
            return new Collection();
        }
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
     * @param options.baseChannel - Base output channel (0-15, default 0)
     * @param options.gain - Output gain
     * @param options.pan - Pan position (-5 = left, 0 = center, +5 = right)
     * @param options.width - Stereo width/spread (0 = no spread, 5 = full spread, default 0)
     */
    out(options: StereoOutOptions = {}): this {
        if (this.items.length > 0) {
            this.items[0].builder.addOut([...this.items], {
                baseChannel: 0,
                ...options,
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

    /**
     * Add self to the send-return bus
     *
     * @param bus
     * @param gain
     * @returns
     */
    send(bus: Bus, gain?: PolySignal): this {
        bus.addSend([...this], gain);
        return this;
    }

    pipe<U>(pipelineFunc: (self: this) => U): U;
    pipe<U extends ModuleOutput | Iterable<ModuleOutput>, E>(
        pipelineFunc: (self: this, item: E) => U,
        array: E[],
    ): Collection;
    pipe<U>(
        pipelineFunc: (self: this, ...args: unknown[]) => U,
        ...arrays: unknown[][]
    ): U | Collection {
        if (arrays.length === 0) {
            return pipelineFunc(this);
        }
        return this.items[0].builder.$c(
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
        mix: PolySignal = 2.5,
    ): Collection {
        const result = pipelineFunc(this);
        return crossfadeMix(this.items[0].builder, this, result, mix);
    }

    /**
     * Chainable module namespace. Every module whose first argument is a
     * (poly)signal becomes a method here, receiving this collection as that
     * argument.
     * @example $c(a, b).$.lpf('100hz')  // ≡ $lpf($c(a, b), '100hz')
     */
    get $(): DollarChainProxy {
        const builder = this.items[0]?.builder;
        return builder
            ? builder.makeDollarChain(this, false)
            : emptyDollarChain();
    }

    /**
     * Like {@link $}, but each method takes a leading `mix` signal that
     * crossfades the dry input against the wet result (0 = dry, 5 = wet,
     * 2.5 = equal).
     * @example $c(a, b).$m.lpf(2.5, '100hz')
     */
    get $m(): DollarChainProxy {
        const builder = this.items[0]?.builder;
        return builder
            ? builder.makeDollarChain(this, true)
            : emptyDollarChain();
    }

    /**
     * Fold this collection's channels down to `channels` output channels by
     * panning them evenly across the output field (equal-power). Builds a
     * \$mixDown module. Defaults to mono.
     */
    mix(
        channels?: number,
        mode?: 'sum' | 'average' | 'max' | 'min',
    ): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$mixDown');
        if (!factory) {
            throw new Error('Factory for $mixDown not registered');
        }
        return factory(
            this.items,
            channels,
            mode !== undefined ? { mode } : undefined,
        ) as Collection;
    }

    /**
     * Wrap every output as a {@link ModuleOutputWithRange} carrying a known
     * value range, returning a {@link CollectionWithRange}. `min`/`max` may be
     * poly: each is normalized and cycled across the items. Internal plumbing
     * for `.range()`; not part of the DSL surface.
     */
    withRange(min: PolySignal, max: PolySignal): CollectionWithRange {
        const mins = toSignalArray(min);
        const maxs = toSignalArray(max);
        return new CollectionWithRange(
            ...this.items.map((o, i) =>
                o.withRange(mins[i % mins.length], maxs[i % maxs.length]),
            ),
        );
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
    /**
     * Remap outputs from explicit input range to output range
     */
    range(
        outMin: PolySignal,
        outMax: PolySignal,
        inMin: PolySignal,
        inMax: PolySignal,
    ): CollectionWithRange {
        if (this.items.length === 0) {
            return new CollectionWithRange();
        }
        const factory = this.items[0].builder.getFactory('$remap');
        if (!factory) {
            throw new Error('Factory for util.remap not registered');
        }
        return (
            factory(this.items, outMin, outMax, inMin, inMax) as Collection
        ).withRange(outMin, outMax);
    }
}

/**
 * Collection of ModuleOutputWithRange instances.
 * Use .range(outMin, outMax) to remap using stored min/max values.
 */
export class CollectionWithRange extends BaseCollection<ModuleOutputWithRange> {
    /** Already ranged: returns itself. */
    withRange(_min: PolySignal, _max: PolySignal): CollectionWithRange {
        return this;
    }

    /**
     * Remap outputs from their known range to a new output range
     */
    range(outMin: PolySignal, outMax: PolySignal): CollectionWithRange {
        if (this.items.length === 0) {
            return new CollectionWithRange();
        }
        const factory = this.items[0].builder.getFactory('$remap');
        if (!factory) {
            throw new Error('Factory for util.remap not registered');
        }
        return (
            factory(
                this.items,
                outMin,
                outMax,
                this.items.map((o) => o.minValue),
                this.items.map((o) => o.maxValue),
            ) as Collection
        ).withRange(outMin, outMax);
    }
}

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
 * Broadcast a signal across `channels` channels by cycling its existing
 * channels (channel `i` ← source channel `i % len`). Useful when a narrow
 * input must excite a wider polyphonic structure: summing/buffering modules
 * skip a narrower input group's missing channels rather than cycling them, so
 * the extra channels would otherwise be silent. Returns the signal unchanged
 * when it carries no channels.
 */
export const cycleToChannels = (
    signal: ModuleOutput | Collection,
    channels: number,
): ModuleOutput | Collection => {
    const items = signal instanceof BaseCollection ? [...signal] : [signal];
    if (items.length === 0) return signal;
    return items[0].builder.$c(
        ...Array.from({ length: channels }, (_, i) => items[i % items.length]),
    );
};

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
    private modules = new Map<string, ModuleSpec>();
    private counters = new Map<string, number>();
    private schemas: ProcessedModuleSchema[] = [];
    private schemaByName = new Map<string, ProcessedModuleSchema>();
    /** Maps a flat `.$.` method name (e.g. `lpf`) to its module name (e.g. `$lpf`). */
    private dollarLookup = new Map<string, string>();
    /**
     * Namespace tree for dotted modules' `.$.` methods: e.g. `$unstable.shape.fold`
     * is reached as `sig.$.unstable.shape.fold(...)`, mirroring the global
     * `$unstable.shape.fold(...)` namespace and avoiding leaf-name collisions with
     * flat modules like `$fold`. Arbitrary nesting depth is supported.
     */
    private dollarNamespaceRoot: DollarNamespaceNode = {
        leaves: new Map(),
        children: new Map(),
    };
    /**
     * `.$.` / `.$m.` methods backed by DSL sugar rather than a module schema
     * (e.g. `delay`). Each takes the chained signal as its first argument and
     * returns the processed result, mirroring a module factory.
     */
    private syntheticDollarMethods = new Map<
        string,
        (
            self: ModuleOutput | BaseCollection<ModuleOutput>,
            ...args: unknown[]
        ) => ModuleOutput | BaseCollection<ModuleOutput>
    >();
    private scopes: ScopeWithLocation[] = [];
    /**
     * Latest call to `$scopeXY` — last-call-wins (only one global XY scope
     * at a time). Resolved during `toPatch` like deferred outputs in scopes.
     */
    private scopeXY: ScopeXYWithLocation | null = null;
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
        for (const schema of this.schemas) {
            if (!qualifiesForDollarChain(schema)) {
                continue;
            }
            const leaf = dollarMethodName(schema.name);
            const base = schema.name.startsWith('$')
                ? schema.name.slice(1)
                : schema.name;
            const segments = base.split('.');
            if (segments.length === 1) {
                this.dollarLookup.set(leaf, schema.name);
            } else {
                // Walk (creating) the namespace path — every segment but the last
                // — then register the leaf method at the terminal node.
                let node = this.dollarNamespaceRoot;
                for (const seg of segments.slice(0, -1)) {
                    let child = node.children.get(seg);
                    if (child === undefined) {
                        child = { leaves: new Map(), children: new Map() };
                        node.children.set(seg, child);
                    }
                    node = child;
                }
                node.leaves.set(leaf, schema.name);
            }
        }
    }

    /**
     * Build a `.$.` (or `.$m.` when `withMix`) chainable-module proxy for
     * `self`. Each qualifying module ($lpf → `.lpf(...)`) becomes a method that
     * injects `self` as the module's first (signal) argument. With `withMix`,
     * the method takes a leading `mix` signal that crossfades dry/wet via
     * {@link crossfadeMix}. Unknown property names and symbols return
     * `undefined`, so `then`/iterator probes stay inert.
     */
    makeDollarChain(
        self: ModuleOutput | BaseCollection<ModuleOutput>,
        withMix: boolean,
    ): DollarChainProxy {
        return new Proxy({} as DollarChainProxy, {
            get: (_target, prop) => {
                if (typeof prop !== 'string') {
                    return undefined;
                }
                const moduleName = this.dollarLookup.get(prop);
                if (moduleName !== undefined) {
                    return this.dollarLeaf(self, withMix, moduleName);
                }
                const synthetic = this.syntheticDollarMethods.get(prop);
                if (synthetic !== undefined) {
                    if (!withMix) {
                        return (...args: unknown[]) => synthetic(self, ...args);
                    }
                    return (mix: PolySignal, ...args: unknown[]) =>
                        crossfadeMix(this, self, synthetic(self, ...args), mix);
                }
                const namespace = this.dollarNamespaceRoot.children.get(prop);
                if (namespace !== undefined) {
                    return this.makeDollarNamespace(self, withMix, namespace);
                }
                return undefined;
            },
        });
    }

    /**
     * A single chainable leaf method: inject `self` as the module's first
     * (signal) argument, or crossfade dry/wet when `withMix`.
     */
    private dollarLeaf(
        self: ModuleOutput | BaseCollection<ModuleOutput>,
        withMix: boolean,
        moduleName: string,
    ) {
        const factory = this.getFactory(moduleName);
        if (!withMix) {
            return (...args: unknown[]) => factory(self, ...args);
        }
        return (mix: PolySignal, ...args: unknown[]) =>
            crossfadeMix(this, self, factory(self, ...args), mix);
    }

    /**
     * A nested proxy for a dotted namespace (e.g. `sig.$.unstable.shape`),
     * resolving leaf methods against the node and recursing into child
     * sub-namespaces so any nesting depth chains correctly.
     */
    private makeDollarNamespace(
        self: ModuleOutput | BaseCollection<ModuleOutput>,
        withMix: boolean,
        node: DollarNamespaceNode,
    ): DollarChainProxy {
        return new Proxy({} as DollarChainProxy, {
            get: (_target, prop) => {
                if (typeof prop !== 'string') {
                    return undefined;
                }
                const moduleName = node.leaves.get(prop);
                if (moduleName !== undefined) {
                    return this.dollarLeaf(self, withMix, moduleName);
                }
                const child = node.children.get(prop);
                if (child !== undefined) {
                    return this.makeDollarNamespace(self, withMix, child);
                }
                return undefined;
            },
        });
    }

    /**
     * Register a `.$.` / `.$m.` method backed by DSL sugar (e.g. `delay`).
     * `fn` receives the chained signal as its first argument, exactly like a
     * module factory injects `self`. Resolved by {@link makeDollarChain}.
     */
    registerDollarMethod(
        name: string,
        fn: (
            self: ModuleOutput | BaseCollection<ModuleOutput>,
            ...args: unknown[]
        ) => ModuleOutput | BaseCollection<ModuleOutput>,
    ): void {
        this.syntheticDollarMethods.set(name, fn);
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
        // Keep incrementing until we find a free one.
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
                column: sourceLocation.column,
                idIsExplicit: Boolean(explicitId),
                line: sourceLocation.line,
            });
        }

        const moduleState: ModuleSpec = {
            id,
            idIsExplicit: Boolean(explicitId),
            moduleType,
            params: {},
        };

        this.modules.set(id, moduleState);
        return new ModuleNode(this, id, moduleType, schema);
    }

    /**
     * Get a module by ID
     */
    getModule(id: string): ModuleSpec | undefined {
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
        this.tempoExplicitlySet = true;
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

    /**
     * Get a factory function by module type name.
     * Returns undefined if factories haven't been registered yet.
     */
    getFactory(moduleType: string): FactoryFunction {
        const factory = this.factoryRegistry.get(moduleType);
        if (!factory) {
            throw new Error(`Factory ${moduleType} not found`);
        }
        return factory;
    }

    /**
     * Create a Collection from outputs. Each argument is flattened into the
     * Collection's channels: ModuleOutputs pass through, iterables (other
     * Collections, arrays) are spread, and bare Signal literals — numbers and
     * note/Hz strings — are lifted into `$signal` modules, so `$c(440, 'c4',
     * osc)` works alongside `$c(osc1, osc2)`. Lifting nests, so scalars inside
     * an array argument are handled too. Lifting a scalar needs the `$signal`
     * factory, hence a builder method.
     */
    $c(...args: (Signal | Iterable<Signal>)[]): Collection {
        // Resolved on the first scalar: a Collection of only ModuleOutputs
        // needs no factory.
        let signal: FactoryFunction | undefined;
        const lift = (value: unknown): ModuleOutput[] => {
            // Scalar literal checked before the iterable branch so a string is
            // one signal rather than spread into characters.
            if (typeof value === 'number' || typeof value === 'string') {
                signal ??= this.getFactory('$signal');
                return [...(signal(value) as Collection)];
            }
            if (value instanceof ModuleOutput) {
                return [value];
            }
            return [...(value as Iterable<unknown>)].flatMap(lift);
        };
        return new Collection(...args.flatMap(lift));
    }

    /**
     * Build the final PatchGraph
     *
     * Note: Uses factory functions for signal/mix modules for consistency,
     * which adds overhead from channel count derivation on every patch build.
     */
    toPatch(): PatchGraph {
        this.processingBusses = true;
        for (const bus of this.busses) {
            // Lock the bus from having more sends applied to it
            bus.lock();
        }
        for (const bus of this.busses) {
            // Its up to the bus callback functions to register themselves
            bus.finalize();
        }

        const signalFactory = this.getFactory('$signal');
        const mixFactory = this.getFactory('$mix');
        const stereoMixerFactory = this.getFactory('$stereoMix');
        const scaleAndShiftFactory = this.getFactory('$scaleAndShift');
        const curveFactory = this.getFactory('$curve');

        // Process output groups and build channel collections
        if (this.outGroups.size > 0) {
            // Collect all channel collections to mix together
            const allChannelCollections: (ModuleOutput | number)[][] = [];

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
                        if (group.gain !== undefined) {
                            const curvedAmp = curveFactory(
                                group.gain,
                                GAIN_CURVE_EXP,
                            );
                            const gained = scaleAndShiftFactory(
                                [mixOut],
                                curvedAmp,
                            ) as Collection;
                            outputSignals = [...gained];
                        } else {
                            outputSignals = [mixOut];
                        }
                    }

                    // Build channel collection with baseChannel silent channels prepended
                    const channelCollection: (ModuleOutput | number)[] = [];

                    // Add silent channels for baseChannel offset
                    for (let i = 0; i < baseChannel; i++) {
                        // Push 0 (Signal::Volts(0.0)) to represent silence
                        channelCollection.push(0);
                    }

                    // Add the actual output signals
                    channelCollection.push(...outputSignals);

                    allChannelCollections.push(channelCollection);
                }
            }
            // Mix all channel collections together using poly mix
            // Each collection contributes to corresponding output channels
            const finalMix = mixFactory(allChannelCollections) as Collection;

            // Apply end of chain processing and global output gain
            this.processingEndOfChain = true;
            const gainedMix = this.endOfChainCb(finalMix).gain(this.outputGain);

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
            rootClock.params.numerator = this.timeSignatureNumerator;
            rootClock.params.denominator = this.timeSignatureDenominator;
            rootClock.params.tempoSet = this.tempoExplicitlySet;
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
            scopes: this.scopes.map((scope) => {
                const resolvedChannels = scope.channels.map((ch) => {
                    const deferredOutput = this.deferredOutputs.get(
                        ch.moduleId,
                    );
                    if (deferredOutput) {
                        const resolved = deferredOutput.resolve();
                        if (resolved === null) {
                            throw new Error(
                                'Unset DeferredModuleOutput used in a scope — call .set(...) on the $deferred() before the end of the script',
                            );
                        }
                        return {
                            channel: ch.channel,
                            moduleId: resolved.moduleId,
                            portName: resolved.portName,
                        };
                    }
                    return ch;
                });
                return {
                    ...scope,
                    channels: resolvedChannels,
                } as ScopeWithLocation;
            }),
            scopeXy: this.resolveScopeXY(),
        };

        if (
            process.env.MODULAR_DEBUG_LOG === '1' ||
            process.env.MODULAR_DEBUG_LOG === 'true'
        ) {
            console.log('Built PatchGraph:', ret);
        }
        return ret;
    }

    /**
     * Reset the builder state
     */
    reset(): void {
        this.modules.clear();
        this.scopes = [];
        this.scopeXY = null;
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

    /**
     * Register module output(s) for mono output routing
     */
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

    /**
     * Resolve the current $scopeXY's channel refs against deferred outputs.
     * Returns undefined if no scope is registered or any leg fails to resolve
     * (matches the per-scope skip behaviour for the multi-channel scope path).
     */
    private resolveScopeXY(): ScopeXy | undefined {
        if (this.scopeXY === null) return undefined;
        const resolveChannel = (ch: ScopeChannel): ScopeChannel | null => {
            const deferred = this.deferredOutputs.get(ch.moduleId);
            if (!deferred) return ch;
            const resolved = deferred.resolve();
            if (!resolved) return null;
            return {
                channel: ch.channel,
                moduleId: resolved.moduleId,
                portName: resolved.portName,
            };
        };
        const resolvedPairs: ScopeXyPair[] = [];
        for (const pair of this.scopeXY.pairs) {
            const x = resolveChannel(pair.x);
            const y = resolveChannel(pair.y);
            if (!x || !y) return undefined;
            resolvedPairs.push({ x, y });
        }
        return {
            pairs: resolvedPairs,
            xRange: this.scopeXY.xRange,
            yRange: this.scopeXY.yRange,
        };
    }

    /**
     * Replace the current $scopeXY (last-call-wins). Pairs are already
     * cycled to a common arity by the caller; this just records the channel
     * refs and per-axis display range. Resolved in `toPatch`.
     */
    setScopeXY(
        pairs: { x: ModuleOutput; y: ModuleOutput }[],
        xRange: [number, number],
        yRange: [number, number],
        sourceLocation?: { line: number; column: number },
    ) {
        this.scopeXY = {
            pairs: pairs.map((p) => ({
                x: {
                    channel: p.x.channel,
                    moduleId: p.x.moduleId,
                    portName: p.x.portName,
                },
                y: {
                    channel: p.y.channel,
                    moduleId: p.y.moduleId,
                    portName: p.y.portName,
                },
            })),
            xRange,
            yRange,
            sourceLocation,
        };
    }
}

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
     * Scale this output by a linear factor (5 = unity, 2.5 = half, 10 = 2x).
     *
     * For perceptual (audio-taper) volume control, use {@link gain} instead.
     */
    amplitude(factor: PolySignal): Collection {
        const factory = this.builder.getFactory('$scaleAndShift');
        return factory(this, factor) as Collection;
    }

    /** Alias for {@link amplitude} */
    amp(factor: PolySignal): Collection {
        return this.amplitude(factor);
    }

    /**
     * Shift this output by an offset
     */
    shift(offset: PolySignal): Collection {
        const factory = this.builder.getFactory('$scaleAndShift');
        return factory(this, undefined, offset) as Collection;
    }

    /**
     * Offset this pitch by an absolute frequency amount, in Hz.
     *
     * The V/Oct signal is converted to Hz, the offset is added, then the
     * result is converted back to V/Oct. Creates an $addHz module.
     */
    addHz(offset: PolySignal): Collection {
        const factory = this.builder.getFactory('$addHz');
        return factory(this, offset) as Collection;
    }

    /**
     * Multiply this pitch by a frequency factor (2 = octave up, 0.5 = down).
     *
     * The V/Oct signal is converted to Hz, multiplied, then converted back
     * to V/Oct. Creates a $mulHz module.
     */
    mulHz(factor: PolySignal): Collection {
        const factory = this.builder.getFactory('$mulHz');
        return factory(this, factor) as Collection;
    }

    /**
     * Scale this output by a factor with a perceptual (audio taper) curve
     * (5 = unity, 0 = silence). Chains $curve → $scaleAndShift with exponent 3.
     *
     * For linear amplitude scaling, use {@link amplitude} instead.
     */
    gain(level: PolySignal): Collection {
        const curveFactory = this.builder.getFactory('$curve');
        const scaleFactory = this.builder.getFactory('$scaleAndShift');
        const curvedLevel = curveFactory(level, GAIN_CURVE_EXP);
        return scaleFactory(this, curvedLevel) as Collection;
    }

    /**
     * Apply a power curve to this output. Creates a $curve module internally.
     */
    exp(factor: PolySignal = GAIN_CURVE_EXP): Collection {
        const factory = this.builder.getFactory('$curve');
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
     * @param options.baseChannel - Base output channel (0-15, default 0)
     * @param options.gain - Output gain (adds util.scaleAndShift after stereo mix)
     * @param options.pan - Pan position (-5 = left, 0 = center, +5 = right)
     * @param options.width - Stereo width/spread (0 = no spread, 5 = full spread, default 0)
     */
    out(options: StereoOutOptions = {}): this {
        this.builder.addOut(this, { baseChannel: 0, ...options });
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

    /**
     * Add self to the send-return bus
     *
     * @param bus
     * @param gain
     * @returns
     */
    send(bus: Bus, gain?: PolySignal): this {
        bus.addSend(this, gain);
        return this;
    }

    pipe<U>(pipelineFunc: (self: this) => U): U;
    pipe<U extends ModuleOutput | Iterable<ModuleOutput>, E>(
        pipelineFunc: (self: this, item: E) => U,
        array: E[],
    ): Collection;
    pipe<U>(
        pipelineFunc: (self: this, ...args: unknown[]) => U,
        ...arrays: unknown[][]
    ): U | Collection {
        if (arrays.length === 0) {
            return pipelineFunc(this);
        }
        return this.builder.$c(
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
        mix: PolySignal = 2.5,
    ): Collection {
        const result = pipelineFunc(this);
        return crossfadeMix(this.builder, this, result, mix);
    }

    /**
     * Chainable module namespace. Every module whose first argument is a
     * (poly)signal becomes a method here, receiving this output as that
     * argument.
     * @example $sine(0).$.lpf('100hz')  // ≡ $lpf($sine(0), '100hz')
     */
    get $(): DollarChainProxy {
        return this.builder.makeDollarChain(this, false);
    }

    /**
     * Like {@link $}, but each method takes a leading `mix` signal that
     * crossfades the dry input against the wet result (0 = dry, 5 = wet,
     * 2.5 = equal).
     * @example $sine(0).$m.lpf(2.5, '100hz')
     */
    get $m(): DollarChainProxy {
        return this.builder.makeDollarChain(this, true);
    }

    /**
     * Fold this output's channels down to `channels` output channels by panning
     * them evenly across the output field (equal-power). Builds a \$mixDown
     * module. Defaults to mono.
     */
    mix(
        channels?: number,
        mode?: 'sum' | 'average' | 'max' | 'min',
    ): Collection {
        const factory = this.builder.getFactory('$mixDown');
        if (!factory) {
            throw new Error('Factory for $mixDown not registered');
        }
        return factory(
            this,
            channels,
            mode !== undefined ? { mode } : undefined,
        ) as Collection;
    }

    /**
     * Wrap this output as a {@link ModuleOutputWithRange} carrying a known value
     * range. Internal plumbing for `.range()`; not part of the DSL surface.
     */
    withRange(min: Signal, max: Signal): ModuleOutputWithRange {
        return new ModuleOutputWithRange(
            this.builder,
            this.moduleId,
            this.portName,
            this.channel,
            min,
            max,
        );
    }

    /**
     * Remap this output from explicit input range to output range
     */
    range(
        outMin: PolySignal,
        outMax: PolySignal,
        inMin: PolySignal,
        inMax: PolySignal,
    ): CollectionWithRange {
        const factory = this.builder.getFactory('$remap');
        return (
            factory(this, outMin, outMax, inMin, inMax) as Collection
        ).withRange(outMin, outMax);
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
    readonly minValue: Signal;
    readonly maxValue: Signal;

    constructor(
        builder: GraphBuilder,
        moduleId: string,
        portName: string,
        channel: number = 0,
        minValue: Signal,
        maxValue: Signal,
    ) {
        super(builder, moduleId, portName, channel);
        this.minValue = minValue;
        this.maxValue = maxValue;
    }

    /** Already ranged: returns itself. */
    withRange(_min: Signal, _max: Signal): ModuleOutputWithRange {
        return this;
    }

    /**
     * Remap this output from its known range to a new range.
     * Creates a remap module internally.
     */
    range(outMin: PolySignal, outMax: PolySignal): CollectionWithRange {
        const factory = this.builder.getFactory('$remap');
        return (
            factory(
                this,
                outMin,
                outMax,
                this.minValue,
                this.maxValue,
            ) as Collection
        ).withRange(outMin, outMax);
    }
}

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
            const resolved = output.resolve();
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

/**
 * Crossfade `original` (dry) against `result` (wet) by `mix` (0 = dry, 5 = wet,
 * 2.5 = equal). The dry leg is amplitude-scaled by `mix` remapped 5→0, the wet
 * leg by `mix` clamped to 0–5, then both sum through `$mix`. Backs both
 * `.pipeMix` and the `.$m.` chainable namespace.
 *
 * Both legs are first broadcast to the wider of the two channel counts via
 * {@link cycleToChannels} so a mismatched dry/wet width is not truncated — `$mix`
 * skips, not cycles, a narrower group's missing channels, so the extra channels
 * would otherwise drop one leg and decay to that of the other alone.
 */
function crossfadeMix(
    builder: GraphBuilder,
    original: Amplifiable,
    result: Amplifiable,
    mix: PolySignal = 2.5,
): Collection {
    const clampFactory = builder.getFactory('$clamp');
    const remapFactory = builder.getFactory('$remap');
    const mixFactory = builder.getFactory('$mix');
    const arity = (a: Amplifiable): number =>
        a instanceof BaseCollection ? Math.max(1, a.length) : 1;
    const channels = Math.max(arity(original), arity(result));
    const dry = cycleToChannels(
        original as ModuleOutput | Collection,
        channels,
    );
    const wet = cycleToChannels(result as ModuleOutput | Collection, channels);
    return mixFactory([
        dry.amplitude(
            clampFactory(remapFactory(mix, 5, 0, 0, 5), { max: 5, min: 0 }),
        ),
        wet.amplitude(clampFactory(mix, { max: 5, min: 0 }) as PolySignal),
    ]) as Collection;
}

/**
 * The `.$`/`.$m` proxy for an empty collection: every method yields an empty
 * `Collection`, matching the empty-collection convention used by the other
 * chainable methods (no builder is available, so no module is instantiated).
 */
function emptyDollarChain(): DollarChainProxy {
    return new Proxy({} as DollarChainProxy, {
        get(_target, prop) {
            if (typeof prop !== 'string') {
                return undefined;
            }
            return () => new Collection();
        },
    });
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

        // Opaque payloads (ParsedPattern from $p(), SpPattern from $p.s(),
        // ArrangePattern from $p.arrange(), and the .fast()/.slow()/.struct()/
        // .beat() wrappers) must be preserved verbatim — walking them would
        // collapse the nulls in `accidental`/`octave`/weight slots to 0 via
        // valueToSignal, producing zero-duration haps and silence. Returning the
        // wrapper verbatim also preserves the nested pattern payloads it carries.
        if (!Array.isArray(replaced)) {
            const kind = (replaced as { __kind?: unknown }).__kind;
            if (
                kind === 'ParsedPattern' ||
                kind === 'SpPattern' ||
                kind === 'ArrangePattern' ||
                kind === 'FastPattern' ||
                kind === 'SlowPattern' ||
                kind === 'StructPattern' ||
                kind === 'BeatPattern'
            ) {
                return replaced;
            }
        }

        if (Array.isArray(replaced)) {
            return replaced
                .map((v, i) => walk(String(i), v))
                .filter((v) => v !== undefined);
        }

        const out: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(replaced)) {
            const entryVal = walk(k, v);
            if (entryVal !== undefined) {
                out[k] = entryVal;
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
        // Opaque pattern payloads (ParsedPattern from $p(), SpPattern from
        // $p.s(), ArrangePattern from $p.arrange(), and the .fast()/.slow()/
        // .struct()/.beat() wrappers) are JSON-only data with no deferred-output
        // strings; mirror the replaceValues short-circuit and return them
        // verbatim instead of deep-walking their mini-notation AST sub-tree.
        const kind = (input as { __kind?: unknown }).__kind;
        if (
            kind === 'ParsedPattern' ||
            kind === 'SpPattern' ||
            kind === 'ArrangePattern' ||
            kind === 'FastPattern' ||
            kind === 'SlowPattern' ||
            kind === 'StructPattern' ||
            kind === 'BeatPattern'
        ) {
            return input;
        }

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
                const output = resolved.resolve();
                if (output === null) {
                    throw new Error(
                        'Unset DeferredModuleOutput used as a module param — call .set(...) on the $deferred() before the end of the script',
                    );
                }
                return valueToSignal(output);
            }
            return maybeResolvedModuleOutput.data;
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
            channel: value.channel,
            module: value.moduleId,
            port: value.portName,
            type: 'cable',
        };
    } else if (value === null || value === undefined) {
        // Silence: 0 becomes Signal::Volts(0.0) in Rust
        return 0;
    }
    // It's a number
    return value;
}
