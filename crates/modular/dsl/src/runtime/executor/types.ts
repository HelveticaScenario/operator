import type { PatchGraph } from '@modular/core';
import type {
    CallSiteSpanRegistry,
    InterpolationResolutionMap,
    SpanAnalyzer,
} from '../types/analysis';
import type { SliderDefinition } from '../../../../../../src/shared/dsl/sliderTypes';
import type { SourceLocation } from '../graph';

/** Result of executing a DSL script. */
export interface DSLExecutionResult {
    patch: PatchGraph;
    sourceLocationMap: Map<string, SourceLocation>;
    interpolationResolutions: InterpolationResolutionMap;
    sliders: SliderDefinition[];
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
        barCount?: number | null;
        loops: Array<{ loopType: string; start: number; end: number }>;
        cuePoints: Array<{ position: number; label: string }>;
        mtime: number;
    };
    /**
     * Optional source span analyzer (e.g. ts-morph based). The runtime calls this
     * before executing user code and threads the result through to factories.
     * If unset, no spans are captured.
     */
    analyzer?: SpanAnalyzer;
}
