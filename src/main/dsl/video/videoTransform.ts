import { VideoOutput } from './VideoOutput';
import { describe, type VideoCore } from './videoBuilderTypes';

/**
 * A node of module `kind` that reads the whole of `input`, which must be a
 * video signal, at coordinates it moves; `values` are its further inputs.
 */
export function transform(
    core: VideoCore,
    fn: string,
    kind: string,
    input: unknown,
    values: Record<string, unknown>,
): VideoOutput {
    if (!(input instanceof VideoOutput)) {
        throw new Error(
            `${fn}: input must be a video signal, got ${describe(input)}`,
        );
    }
    return core.addNode(kind, {
        input: input.value,
        ...core.values(fn, values),
    });
}
