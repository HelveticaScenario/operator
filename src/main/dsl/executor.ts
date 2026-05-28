import type { ModuleSchema, PatchGraph } from '@modular/core';
import { deriveChannelCount } from '@modular/core';
import {
    DSLContext,
    hz,
    note,
    setActiveSpanRegistry,
    setDSLWrapperLineOffset,
    captureSourceLocation,
} from './factories';
import type {
    BufferOutputRef,
    Signal,
    SourceLocation,
    Collection,
    ModuleOutput,
    CollectionWithRange,
} from './GraphBuilder';
import {
    $c,
    $r,
    $cartesian,
    DeferredModuleOutput,
    DeferredCollection,
    Bus,
    replaceSignals,
} from './GraphBuilder';
import { analyzeSourceSpans } from './analyzeSource';
import type { CallSiteSpanRegistry } from './analyzeSource';
import type { InterpolationResolutionMap } from '../../shared/dsl/spanTypes';
import { setActiveInterpolationResolutions } from '../../shared/dsl/spanTypes';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import { $p } from './miniNotation';

// Augment Array.prototype with pipe() for TypeScript
declare global {
    interface Array<T> {
        pipe<U>(this: this, pipelineFunc: (self: this) => U): U;
    }
}

/**
 * Result of executing a DSL script.
 */
export interface DSLExecutionResult {
    /** The generated patch graph */
    patch: PatchGraph;
    /** Map from module ID to source location in DSL code */
    sourceLocationMap: Map<string, SourceLocation>;
    /** Interpolation resolution map for template literal const redirects */
    interpolationResolutions: InterpolationResolutionMap;
    /** Slider definitions created by $slider() DSL function calls */
    sliders: SliderDefinition[];
    /** Full call expression spans for DSL methods (.scope(), $slider(), etc.) */
    callSiteSpans: CallSiteSpanRegistry;
}

export interface WavsFolderNode {
    [name: string]: WavsFolderNode | 'file';
}

export interface DSLExecutionOptions {
    sampleRate?: number;
    workspaceRoot?: string | null;
    wavsFolderTree?: WavsFolderNode | null;
    loadWav?: (path: string) => {
        channels: number;
        frameCount: number;
        path: string;
        sampleRate: number;
        duration: number;
        bitDepth: number;
        pitch?: number | null;
        playback?: string | null;
        bpm?: number | null;
        beats?: number | null;
        timeSignature?: { num: number; den: number } | null;
        loops: Array<{ loopType: string; start: number; end: number }>;
        cuePoints: Array<{ position: number; label: string }>;
        mtime: number;
    };
}

// Install pipe() on Array.prototype so arrays in the DSL can use it.
// Non-enumerable to avoid polluting for-in loops.
if (typeof Array.prototype.pipe !== 'function') {
    Object.defineProperty(Array.prototype, 'pipe', {
        configurable: true,
        enumerable: false,
        value: function pipe<T>(
            this: unknown,
            pipelineFunc: (self: typeof this) => T,
        ): T {
            return pipelineFunc(this);
        },
        writable: true,
    });
}

/**
 * Execute a DSL script and return the resulting PatchGraph with source locations.
 */
export function executePatchScript(
    source: string,
    schemas: ModuleSchema[],
    options: DSLExecutionOptions = {},
): DSLExecutionResult {
    // Create DSL context
    // Console.log('Executing DSL script with schemas:', schemas);
    const context = new DSLContext(schemas);
    const sampleRate = options.sampleRate ?? 48_000;

    // Create the execution environment with all DSL functions
    // Remove _clock from user-facing namespace (it's internal, used only for ROOT_CLOCK)
    const { _clock, ...userNamespaceTree } = context.namespaceTree;

    if (typeof _clock !== 'function') {
        throw new Error(
            'DSL execution error: "_clock" module not found in schemas',
        );
    }

    const signal = context.namespaceTree['$signal'];
    if (typeof signal !== 'function') {
        throw new Error(
            'DSL execution error: "$signal" module not found in schemas',
        );
    }

    // Create default clock module that runs at 120 BPM
    const $clock = _clock(120, 4, 4, {
        id: 'ROOT_CLOCK',
    });

    const rootInput = signal(
        Array.from({ length: 16 }, (_, i) => ({
            channel: i,
            module: 'HIDDEN_AUDIO_IN',
            port: 'input',
            type: 'cable',
        })),
        { id: 'ROOT_INPUT' },
    );

    // Create functions to set global tempo and output gain
    const builder = context.getBuilder();
    const $setTempo = (tempo: number) => {
        builder.setTempo(tempo);
    };
    const $setOutputGain = (gain: Signal) => {
        builder.setOutputGain(gain);
    };
    const $setTimeSignature = (numerator: number, denominator: number) => {
        if (!Number.isInteger(numerator) || numerator < 1) {
            throw new Error(
                `$setTimeSignature: numerator must be a positive integer, got ${numerator}`,
            );
        }
        if (!Number.isInteger(denominator) || denominator < 1) {
            throw new Error(
                `$setTimeSignature: denominator must be a positive integer, got ${denominator}`,
            );
        }
        builder.setTimeSignature(numerator, denominator);
    };

    /**
     * Create a DeferredCollection with placeholder signals that can be assigned later.
     * Useful for feedback loops and forward references.
     * @param channels - Number of deferred outputs (1-16, default 1)
     */
    const $deferred = (channels: number = 1): DeferredCollection => {
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

    const $bus = (cb: (mixed: Collection) => unknown): Bus =>
        new Bus(builder, cb);

    const $setEndOfChainCb = (
        cb: (
            mixed: Collection,
        ) => ModuleOutput | Collection | CollectionWithRange,
    ) => {
        builder.setEndOfChainCb(cb);
    };

    const $buffer = (
        input: ModuleOutput | Collection | number,
        lengthSeconds: number,
        config?: { id?: string },
    ): BufferOutputRef => {
        if (
            typeof lengthSeconds !== 'number' ||
            !Number.isFinite(lengthSeconds)
        ) {
            throw new Error('$buffer() lengthSeconds must be a finite number');
        }
        if (lengthSeconds <= 0) {
            throw new Error(
                `$buffer() lengthSeconds must be greater than 0, got ${lengthSeconds}`,
            );
        }

        const sourceLocation = captureSourceLocation();

        // Create a $buffer module in the graph
        const node = builder.addModule('$buffer', config?.id, sourceLocation);

        // Resolve the input signal and set params
        const resolvedInput = replaceSignals(input);
        node._setParam('input', resolvedInput);
        node._setParam('length', lengthSeconds);

        // Derive channel count from the input signal
        const deriveResult = deriveChannelCount(
            '$buffer',
            node.getParamsSnapshot(),
        );

        if (deriveResult.errors && deriveResult.errors.length > 0) {
            const messages = deriveResult.errors
                .map((e: { message: string }) => e.message)
                .join('; ');
            const loc = sourceLocation ? ` at line ${sourceLocation.line}` : '';
            throw new Error(`$buffer${loc}: ${messages}`);
        }

        const channels =
            deriveResult.channelCount != null ? deriveResult.channelCount : 1;

        if (deriveResult.channelCount != null) {
            node._setDerivedChannelCount(deriveResult.channelCount);
        }

        const frameCount = Math.max(1, Math.ceil(lengthSeconds * sampleRate));

        return {
            type: 'buffer_ref',
            module: node.id,
            port: 'buffer',
            channels,
            frameCount,
        };
    };

    // Slider collector — populated by $slider() calls during execution
    const sliders: SliderDefinition[] = [];

    /**
     * Create a slider control: a signal module with a UI slider bound to it.
     * @param label - Display label (must be a string literal)
     * @param value - Initial value (must be a numeric literal)
     * @param min - Minimum value
     * @param max - Maximum value
     * @returns The signal module's output
     */
    const $slider = (
        label: string,
        value: number,
        min: number,
        max: number,
    ) => {
        if (typeof label !== 'string') {
            throw new Error('$slider() label must be a string literal');
        }
        if (sliders.find((s) => s.label === label)) {
            throw new Error(`$slider() label "${label}" must be unique`);
        }
        if (typeof value !== 'number' || !isFinite(value)) {
            throw new Error('$slider() value must be a finite number literal');
        }
        if (typeof min !== 'number' || !isFinite(min)) {
            throw new Error('$slider() min must be a finite number');
        }
        if (typeof max !== 'number' || !isFinite(max)) {
            throw new Error('$slider() max must be a finite number');
        }
        if (min >= max) {
            throw new Error(
                `$slider() min (${min}) must be less than max (${max})`,
            );
        }

        const moduleId = `__slider_${label.replace(/[^a-zA-Z0-9_]/g, '_')}`;

        // Create backing signal module via the existing signal factory
        const result = signal(value, { id: moduleId });

        sliders.push({ label, max, min, moduleId, value });

        return result;
    };

    /**
     * Load WAV samples from the wavs/ folder.
     * Returns a proxy tree matching the folder structure; leaf nodes trigger
     * loadWav() and return `{ type: 'wav_ref', path, channels }` objects.
     */
    const $wavs = (): unknown => {
        const tree = options.wavsFolderTree;
        if (!tree) {
            return new Proxy(
                {},
                {
                    get(_target, prop) {
                        throw new Error(
                            `$wavs().${String(prop)}: no wavs/ folder found in workspace`,
                        );
                    },
                },
            );
        }

        // Memoize the lexicographically-sorted list of direct file basenames
        // per folder node, so numeric-index resolution sorts at most once
        // per node regardless of how many `$wavs()[i]` calls happen.
        const sortedFilesCache = new WeakMap<WavsFolderNode, string[]>();
        function sortedFileList(node: WavsFolderNode): string[] {
            const cached = sortedFilesCache.get(node);
            if (cached) return cached;
            const files = Object.entries(node)
                .filter(([, v]) => v === 'file')
                .map(([k]) => k)
                .sort((a, b) => a.localeCompare(b));
            sortedFilesCache.set(node, files);
            return files;
        }

        function makeProxy(node: WavsFolderNode, pathParts: string[]): unknown {
            // Resolve a known file leaf (basename `fileName` exists in `node`
            // as a `'file'`) into a `WavHandle`. Single source of truth
            // shared by named-key access and numeric-index access.
            function loadFile(fileName: string): unknown {
                const relPath = [...pathParts, fileName].join('/');
                if (!options.loadWav) {
                    throw new Error('$wavs(): loadWav function not provided');
                }
                const info = options.loadWav(relPath);
                return {
                    type: 'wav_ref' as const,
                    path: relPath,
                    channels: info.channels,
                    sampleRate: info.sampleRate,
                    frameCount: info.frameCount,
                    duration: info.duration,
                    bitDepth: info.bitDepth,
                    mtime: info.mtime,
                    ...(info.pitch != null && { pitch: info.pitch }),
                    ...(info.playback != null && { playback: info.playback }),
                    ...(info.bpm != null && { bpm: info.bpm }),
                    ...(info.beats != null && { beats: info.beats }),
                    ...(info.timeSignature != null && {
                        timeSignature: {
                            num: info.timeSignature.num,
                            den: info.timeSignature.den,
                        },
                    }),
                    loops: info.loops.map(
                        (l: {
                            loopType: string;
                            start: number;
                            end: number;
                        }) => ({
                            type: l.loopType as
                                | 'forward'
                                | 'pingpong'
                                | 'backward',
                            start: l.start,
                            end: l.end,
                        }),
                    ),
                    cuePoints: info.cuePoints.map(
                        (c: { position: number; label: string }) => ({
                            position: c.position,
                            label: c.label,
                        }),
                    ),
                };
            }

            return new Proxy(
                {},
                {
                    get(_target, prop) {
                        if (typeof prop !== 'string') return undefined;

                        // Numeric index access wraps modulo the file count of
                        // this folder. Only direct files participate
                        // (subfolders excluded — they get their own index).
                        if (/^-?(0|[1-9][0-9]*)$/.test(prop)) {
                            const files = sortedFileList(node);
                            if (files.length === 0) {
                                const fullPath = [
                                    ...pathParts,
                                    `[${prop}]`,
                                ].join('/');
                                throw new Error(
                                    `$wavs(): "${fullPath}" — no wav files in this folder to index into`,
                                );
                            }
                            const i = parseInt(prop, 10);
                            const wrapped =
                                ((i % files.length) + files.length) %
                                files.length;
                            return loadFile(files[wrapped]);
                        }

                        const child = node[prop];
                        if (child === undefined) {
                            const fullPath = [...pathParts, prop].join('/');
                            throw new Error(
                                `$wavs(): "${fullPath}" not found. Available: ${Object.keys(node).join(', ') || '(empty)'}`,
                            );
                        }

                        if (child === 'file') {
                            return loadFile(prop);
                        }

                        // Directory node — return nested proxy
                        return makeProxy(child, [...pathParts, prop]);
                    },
                    ownKeys() {
                        return Object.keys(node);
                    },
                    getOwnPropertyDescriptor(_target, prop) {
                        if (typeof prop === 'string' && prop in node) {
                            return {
                                configurable: true,
                                enumerable: true,
                                writable: false,
                            };
                        }
                        return undefined;
                    },
                },
            );
        }

        return makeProxy(tree, []);
    };

    /**
     * $table.* DSL helpers produce phase-warp table descriptors for the
     * `$wavetable` oscillator (and any future modules that accept a `Table`).
     *
     * Each helper returns a plain JSON object whose shape matches the Rust
     * `Table` enum deserializer (`#[serde(tag = "type", rename_all = "camelCase")]`).
     *
     * Inner signal-valued fields are passed through `replaceSignals` so that
     * ModuleOutputs / Collections are converted to the same wire format used
     * for module-factory params. This matches the existing mechanism used by
     * `_setParam` in GraphBuilder.
     *
     * Tables are composable: each returned descriptor has a `.pipe(next)` method
     * that feeds this table's output phase into `next`. The optional second
     * argument to each helper is a shorthand for `.pipe(next)`.
     */
    function wrapTable(descriptor: Record<string, unknown>): Record<
        string,
        unknown
    > & {
        pipe: <T>(fn: (self: Record<string, unknown>) => T) => T;
    } {
        const t = { ...descriptor } as Record<string, unknown> & {
            pipe: <T>(fn: (self: Record<string, unknown>) => T) => T;
        };
        Object.defineProperty(t, 'pipe', {
            value: <T>(fn: (self: typeof t) => T): T => fn(t),
            enumerable: false,
            writable: false,
            configurable: false,
        });
        return t;
    }

    const $table = {
        mirror: (amount: unknown, next?: unknown) => {
            const t = wrapTable({
                type: 'mirror',
                amount: replaceSignals(amount),
            });
            return next !== undefined
                ? wrapTable({ type: 'pipe', first: t, second: next })
                : t;
        },
        bend: (amount: unknown, next?: unknown) => {
            const t = wrapTable({
                type: 'bend',
                amount: replaceSignals(amount),
            });
            return next !== undefined
                ? wrapTable({ type: 'pipe', first: t, second: next })
                : t;
        },
        sync: (ratio: unknown, next?: unknown) => {
            const t = wrapTable({ type: 'sync', ratio: replaceSignals(ratio) });
            return next !== undefined
                ? wrapTable({ type: 'pipe', first: t, second: next })
                : t;
        },
        fold: (amount: unknown, next?: unknown) => {
            const t = wrapTable({
                type: 'fold',
                amount: replaceSignals(amount),
            });
            return next !== undefined
                ? wrapTable({ type: 'pipe', first: t, second: next })
                : t;
        },
        pwm: (width: unknown, next?: unknown) => {
            const t = wrapTable({ type: 'pwm', width: replaceSignals(width) });
            return next !== undefined
                ? wrapTable({ type: 'pipe', first: t, second: next })
                : t;
        },
    };

    const dslGlobals = {
        // Prefixed namespace tree (modules and namespaces, minus _clock)
        ...userNamespaceTree,
        // Helper functions with $ prefix
        $hz: hz,
        $note: note,
        // Mini-notation parser — wraps a string in a ParsedPattern that
        // $cycle / $iCycle consume as a positional argument.
        $p,
        // Phase-warp table descriptors for $wavetable
        $table,
        // Collection helpers
        $c,
        $r,
        $cartesian,
        // Deferred signal helper
        $deferred,
        // Slider control
        $slider,
        // Bus
        $bus,
        // Global settings
        $setTempo,
        $setOutputGain,
        $setTimeSignature,
        $setEndOfChainCb,
        $buffer,
        // WAV sample loading
        $wavs,
        // Built-in modules
        $clock,
        $input: rootInput,
    };

    // Console.log(dslGlobals);

    // Build the function body - count wrapper lines for source mapping
    // When new Function() executes code, line numbers in stack traces are relative
    // To the function body string. The template literal structure plus new Function's
    // Own wrapper results in user code starting at line 5 in stack traces.
    const wrapperLineCount = 4;
    setDSLWrapperLineOffset(wrapperLineCount);

    // The function body template indents the first line of source with 4 spaces
    // This affects the column reported by V8 for the first line only
    const firstLineColumnOffset = 4;

    // Analyze source code to extract argument spans before execution
    // The registry maps call-site keys (line:column) to argument span info
    const {
        registry: spanRegistry,
        interpolationResolutions,
        callSiteSpans,
    } = analyzeSourceSpans(
        source,
        schemas,
        wrapperLineCount,
        firstLineColumnOffset,
    );
    setActiveSpanRegistry(spanRegistry);
    setActiveInterpolationResolutions(interpolationResolutions);

    const functionBody = `
    'use strict';
    ${source}
  `;

    // Create parameter names and values
    const paramNames = Object.keys(dslGlobals);
    const paramValues = Object.values(dslGlobals);

    try {
        // Execute the script
        const fn = new Function(...paramNames, functionBody);
        fn(...paramValues);

        // Build and return the patch with source locations
        const resultBuilder = context.getBuilder();
        const patch = resultBuilder.toPatch();
        const sourceLocationMap = resultBuilder.getSourceLocationMap();

        return {
            callSiteSpans,
            interpolationResolutions,
            patch,
            sliders,
            sourceLocationMap,
        };
    } catch (error) {
        if (error instanceof Error) {
            throw new Error(`DSL execution error: ${error.message}`, {
                cause: error,
            });
        }
        throw error;
    } finally {
        // Clear the span registry after execution — spans are already baked into
        // Module state via ARGUMENT_SPANS_KEY so the registry isn't needed anymore.
        setActiveSpanRegistry(null);
        // NOTE: Do NOT clear interpolation resolutions here. They are read
        // Asynchronously by moduleStateTracking during decoration polling and
        // Must persist until the next execution replaces them.
    }
}

/**
 * Validate DSL script syntax without executing
 */
export function validateDSLSyntax(source: string): {
    valid: boolean;
    error?: string;
} {
    try {
        // Create function only for syntax validation - not executed
        const _fn = new Function(source);
        return { valid: true };
    } catch (error) {
        if (error instanceof Error) {
            return { error: error.message, valid: false };
        }
        return { error: 'Unknown syntax error', valid: false };
    }
}
