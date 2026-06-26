/**
 * Compare two dotted version strings (e.g. `"0.0.101"`). Returns a negative
 * number when `a` is older than `b`, `0` when they are equal, and a positive
 * number when `a` is newer.
 *
 * Components are compared left-to-right as integers; a missing component counts
 * as `0`, so `"0.1"` equals `"0.1.0"`. Only the numeric release core is
 * compared — any `-suffix` (a pre-release tag) is dropped before parsing.
 * Pre-release ordering is not modelled; the app ships plain `major.minor.patch`
 * versions.
 */
export function compareVersion(a: string, b: string): number {
    const pa = parseParts(a);
    const pb = parseParts(b);
    const len = Math.max(pa.length, pb.length);
    for (let i = 0; i < len; i++) {
        const diff = (pa[i] ?? 0) - (pb[i] ?? 0);
        if (diff !== 0) return diff < 0 ? -1 : 1;
    }
    return 0;
}

function parseParts(version: string): number[] {
    return version
        .split('-')[0]
        .split('.')
        .map((part) => {
            const n = parseInt(part, 10);
            return Number.isFinite(n) ? n : 0;
        });
}
