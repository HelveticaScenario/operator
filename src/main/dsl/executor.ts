import { ModuleSchema, PatchGraph } from '@modular/core';
import {
    DSLContext,
    hz,
    note,
    setDSLWrapperLineOffset,
    setActiveSpanRegistry,
} from './factories';
import {
    $c,
    $r,
    $cartesian,
    Signal,
    SourceLocation,
    DeferredModuleOutput,
    DeferredCollection,
} from './GraphBuilder';
import { analyzeSourceSpans } from './analyzeSource';
import type { CallSiteSpanRegistry } from './analyzeSource';
import type { InterpolationResolutionMap } from '../../shared/dsl/spanTypes';
import { setActiveInterpolationResolutions } from '../../shared/dsl/spanTypes';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';

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

// Install pipe() on Array.prototype so arrays in the DSL can use it.
// Non-enumerable to avoid polluting for-in loops.
if (typeof Array.prototype.pipe !== 'function') {
    Object.defineProperty(Array.prototype, 'pipe', {
        value: function pipe<T>(
            this: unknown,
            pipelineFunc: (self: typeof this) => T,
        ): T {
            return pipelineFunc(this);
        },
        writable: true,
        configurable: true,
        enumerable: false,
    });
}

/**
 * Execute a DSL script and return the resulting PatchGraph with source locations.
 */
export function executePatchScript(
    source: string,
    schemas: ModuleSchema[],
): DSLExecutionResult {
    // Create DSL context
    // console.log('Executing DSL script with schemas:', schemas);
    const context = new DSLContext(schemas);

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
            type: 'cable',
            module: 'HIDDEN_AUDIO_IN',
            port: 'input',
            channel: i,
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

        sliders.push({ moduleId, label, value, min, max });

        return result;
    };

    const dslGlobals = {
        // Prefixed namespace tree (modules and namespaces, minus _clock)
        ...userNamespaceTree,
        // Helper functions with $ prefix
        $hz: hz,
        $note: note,
        // Collection helpers
        $c,
        $r,
        $cartesian,
        // Deferred signal helper
        $deferred,
        // Slider control
        $slider,
        // Global settings
        $setTempo,
        $setOutputGain,
        $setTimeSignature,
        // Built-in modules
        $clock,
        $input: rootInput,
    };

    // console.log(dslGlobals);

    // Build the function body - count wrapper lines for source mapping
    // When new Function() executes code, line numbers in stack traces are relative
    // to the function body string. The template literal structure plus new Function's
    // own wrapper results in user code starting at line 5 in stack traces.
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
        const builder = context.getBuilder();
        const patch = builder.toPatch();
        const sourceLocationMap = builder.getSourceLocationMap();

        return {
            patch,
            sourceLocationMap,
            interpolationResolutions,
            sliders,
            callSiteSpans,
        };
    } catch (error) {
        if (error instanceof Error) {
            throw new Error(`DSL execution error: ${error.message}`);
        }
        throw error;
    } finally {
        // Clear the span registry after execution — spans are already baked into
        // module state via ARGUMENT_SPANS_KEY so the registry isn't needed anymore.
        setActiveSpanRegistry(null);
        // NOTE: Do NOT clear interpolation resolutions here. They are read
        // asynchronously by moduleStateTracking during decoration polling and
        // must persist until the next execution replaces them.
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
        new Function(source);
        return { valid: true };
    } catch (error) {
        if (error instanceof Error) {
            return { valid: false, error: error.message };
        }
        return { valid: false, error: 'Unknown syntax error' };
    }
}
