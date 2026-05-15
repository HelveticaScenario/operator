import type { ModuleSchema } from '@modular/core';
import type { SliderDefinition } from '../../../shared/dsl/sliderTypes';
import { $cartesian, $c, $r } from '../graph';
import { DSLContext, hz, note } from '../factory';
import { create$bus } from '../composites/$bus';
import { create$buffer } from '../composites/$buffer';
import { create$delay } from '../composites/$delay';
import { create$deferred } from '../composites/$deferred';
import { create$slider } from '../composites/$slider';
import { create$ott } from '../composites/$ott';
import { createSettings } from '../composites/settings';
import { create$wavs } from '../wavs/proxy';
import { $table } from '../table/$table';
import type { DSLExecutionOptions } from './types';

/** Build dslGlobals + supporting state for a single execution. */
export function buildContext(
    schemas: ModuleSchema[],
    options: DSLExecutionOptions,
) {
    const context = new DSLContext(schemas);
    const sampleRate = options.sampleRate ?? 48_000;
    const builder = context.getBuilder();

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

    const $mix = userNamespaceTree['$mix'];
    if (typeof $mix !== 'function') {
        throw new Error(
            'DSL execution error: "$mix" module not found in schemas',
        );
    }

    // Default clock module that runs at 120 BPM
    const $clock = _clock(120, 4, 4, { id: 'ROOT_CLOCK' });

    const rootInput = signal(
        Array.from({ length: 16 }, (_, i) => ({
            channel: i,
            module: 'HIDDEN_AUDIO_IN',
            port: 'input',
            type: 'cable',
        })),
        { id: 'ROOT_INPUT' },
    );

    const sliders: SliderDefinition[] = [];

    const $deferred = create$deferred(builder);
    const $bus = create$bus(builder);
    const $buffer = create$buffer(builder, sampleRate);
    const $delay = create$delay({ $buffer, $deferred, $mix });
    const $slider = create$slider(sliders, signal);
    const $ott = create$ott({ namespaceTree: userNamespaceTree, $mix });
    const settings = createSettings(builder);
    const $wavs = create$wavs(options);

    const dslGlobals = {
        // Prefixed namespace tree (modules and namespaces, minus _clock)
        ...userNamespaceTree,
        // Helper functions with $ prefix
        $hz: hz,
        $note: note,
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
        ...settings,
        $buffer,
        $delay,
        // 3-band compressor composite
        $ott,
        // WAV sample loading
        $wavs,
        // Built-in modules
        $clock,
        $input: rootInput,
    };

    return { context, dslGlobals, sliders };
}
