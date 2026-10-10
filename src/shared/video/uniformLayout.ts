/**
 * Float offsets into the shader's uniform buffer: `time`, then `resolution`
 * (a vec2), then the slot array that control-bound inputs read.
 */
export const UNIFORM_TIME_OFFSET = 0;
export const UNIFORM_RESOLUTION_OFFSET = 2;
export const UNIFORM_SLOTS_OFFSET = 4;

/**
 * Bind group slots of a shader's resources: the uniform buffer is 0, the sampler
 * 1, then one texture per feedback buffer, one for audio history when there is
 * any, and one per media source.
 */
export function bindingSlots(
    bufferCount: number,
    historyCount: number,
): { sampler: number; buffer: number; history: number; source: number } {
    const buffer = 2;
    const history = buffer + bufferCount;
    return {
        buffer,
        history,
        sampler: 1,
        source: history + (historyCount > 0 ? 1 : 0),
    };
}
