import { deriveChannelCount } from '@modular/core';
import type { BufferOutputRef, Collection, GraphBuilder, ModuleOutput } from '../graph';
import { replaceSignals } from '../graph';
import { captureSourceLocation } from '../captureSourceLocation';

/**
 * `$buffer(input, lengthSeconds, config?)` — allocate a circular buffer that
 * records `input` and returns a buffer reference for downstream readers.
 */
export function create$buffer(builder: GraphBuilder, sampleRate: number) {
    return (
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

        const node = builder.addModule('$buffer', config?.id, sourceLocation);

        const resolvedInput = replaceSignals(input);
        node._setParam('input', resolvedInput);
        node._setParam('length', lengthSeconds);

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
}
