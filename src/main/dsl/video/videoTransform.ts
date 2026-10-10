import type { VideoValue } from '../../../shared/video/videoGraph';
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
    const inputs: Record<string, VideoValue> = { input: input.value };
    for (const [name, value] of Object.entries(fields)) {
        inputs[name] = core.asField(fn, name, value);
    }
    return core.addNode(
        input.type === 'color' ? `${kind}Color` : kind,
        input.type,
        inputs,
    );
}
