/**
 * Fills `into` with the samples to show from `recent`, the latest samples
 * oldest first, and returns it. With `trigger`, the window starts at the first
 * rising zero crossing that leaves a full window after it, so a periodic wave
 * holds still from frame to frame; with no such crossing, or too few samples
 * to search, it is simply the newest `into.length`.
 */
export function alignWindow<T extends Float32Array>(
    recent: ArrayLike<number>,
    into: T,
    trigger: boolean,
): T {
    const count = into.length;
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
    for (let i = 0; i < count; i++) into[i] = recent[start + i] ?? 0;
    return into;
}
