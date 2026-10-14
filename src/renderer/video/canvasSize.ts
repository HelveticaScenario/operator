/**
 * `width` x `height` scaled down, keeping its aspect ratio, so neither side
 * exceeds `max`, the largest texture the GPU device allows.
 */
export function withinTextureLimit(
    width: number,
    height: number,
    max: number,
): { height: number; width: number } {
    const scale = max / Math.max(width, height);
    if (scale >= 1) return { height, width };
    return {
        height: Math.max(1, Math.min(max, Math.round(height * scale))),
        width: Math.max(1, Math.min(max, Math.round(width * scale))),
    };
}
