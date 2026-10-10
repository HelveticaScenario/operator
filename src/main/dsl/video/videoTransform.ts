import { VideoOutput } from './VideoOutput';
import { describe, type VideoCore } from './videoBuilderTypes';

/**
 * A transform of `input`: its field or color variant is chosen by the input's
 * type, and `fields` are the extra inputs, always fields.
 */
export function transform(
    core: VideoCore,
    fn: string,
    kind: string,
    input: unknown,
    fields: Record<string, unknown>,
): VideoOutput {
    if (!(input instanceof VideoOutput)) {
        throw new Error(
            `${fn}: input must be a video field or color, got ${describe(input)}`,
        );
    }
    return core.addNode(
        input.type === 'color' ? `${kind}Color` : kind,
        input.type,
        { input: input.value, ...core.fields(fn, fields) },
    );
}
