/**
 * Internals shared between `migrateCycleCalls` and its comment-rewrite
 * helper. Splitting these out breaks an import cycle — both call sites
 * import from here instead of each other.
 */

export interface Edit {
    start: number;
    end: number;
    replacement: string;
}

// Legacy voltage atoms wrote the unit suffix `v` after a number (`5v`,
// `0.5v`, `-3v`). The current grammar reads a bare number as the voltage,
// so the suffix is dropped. The number matches the grammar's `Number`
// (`-?\d+(\.\d+)?`); boundary guards keep note octaves (`c5v`) and longer
// identifiers (`5val`) intact. The `v` is case-insensitive.
const VOLT_SUFFIX_RE = /(?<![\w.])(-?\d+(?:\.\d+)?)[vV](?![\w.])/g;

export function stripVolts(text: string): string {
    return text.replace(VOLT_SUFFIX_RE, '$1');
}

export function wrapP(start: number, end: number, text: string): Edit {
    return { start, end, replacement: `$p(${stripVolts(text)})` };
}

export function buildSpReplacement(sources: string[], scale: string): string {
    const [head, ...rest] = sources;
    const chain = rest.map((rhs) => `.add(${rhs})`).join('');
    return `$cycle($sp(${head}, ${scale})${chain})`;
}
