import type { Scope } from '@modular/core';
import z from 'zod';
import type { ModuleOutput } from './moduleOutput';

export const PORT_MAX_CHANNELS = 16;

/** Exponent used by .gain() for perceptual amplitude curve */
export const GAIN_CURVE_EXP = 3;

/**
 * Scope with an optional source location captured at call time.
 * The base Scope type comes from Rust (napi); the extra field is
 * ignored by Rust but flows through to the renderer via IPC.
 */
export type ScopeWithLocation = Scope & {
    sourceLocation?: { line: number; column: number };
};

export interface OutputSchemaWithRange {
    name: string;
    description: string;
    polyphonic?: boolean;
    minValue?: number;
    maxValue?: number;
}

export const ResolvedModuleOutputSchema = z.object({
    channel: z.number().optional(),
    module: z.string(),
    port: z.string(),
    type: z.literal('cable'),
});

export type ResolvedModuleOutput = z.infer<typeof ResolvedModuleOutputSchema>;

export type OrArray<T> = T | T[];
export type Signal = number | string | ModuleOutput;
export type PolySignal = OrArray<Signal> | Iterable<ModuleOutput>;

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

export interface SendGroup {
    outputs: ModuleOutput[];
    gain?: PolySignal;
}

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
 * Factory function type for creating modules via DSL.
 * Returns the module's output(s) directly rather than the ModuleNode.
 */
export type FactoryFunction = (
    ...args: unknown[]
) => unknown;
