/**
 * Wire-shape discriminators of every pattern value a signal accepts: `$p(...)`,
 * `$p.s(...)`, `$p.arrange(...)` and the `.fast`/`.slow`/`.struct`/`.beat`
 * chains built from them.
 */
export const PATTERN_KINDS: ReadonlySet<unknown> = new Set([
    'ParsedPattern',
    'SpPattern',
    'ArrangePattern',
    'FastPattern',
    'SlowPattern',
    'StructPattern',
    'BeatPattern',
]);

export interface PatternValue {
    readonly __kind: string;
}

export function isPatternValue(value: unknown): value is PatternValue {
    return (
        typeof value === 'object' &&
        value !== null &&
        PATTERN_KINDS.has((value as { __kind?: unknown }).__kind)
    );
}
