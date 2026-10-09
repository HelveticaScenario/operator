/**
 * Picks the `count` samples to show from `recent`, the latest samples oldest
 * first. With `trigger`, the window starts at the first rising zero crossing
 * that leaves a full window after it, so a periodic wave holds still from frame
 * to frame; with no such crossing, or too few samples to search, it is simply
 * the newest `count`.
 */
export function alignWindow(
    recent: ArrayLike<number>,
    count: number,
    trigger: boolean,
): Float32Array<ArrayBuffer> {
    const latest = Math.max(0, recent.length - count);
    let start = latest;
    if (trigger) {
        for (let i = 1; i <= latest; i++) {
            if (recent[i - 1] < 0 && recent[i] >= 0) {
                start = i;
                break;
            }
        }
    }
    const window = new Float32Array(count);
    for (let i = 0; i < count; i++) window[i] = recent[start + i] ?? 0;
    return window;
}
