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

export function wrapP(start: number, end: number, text: string): Edit {
    return { start, end, replacement: `$p(${text})` };
}

export function buildSpReplacement(sources: string[], scale: string): string {
    const [head, ...rest] = sources;
    const chain = rest.map((rhs) => `.add(${rhs})`).join('');
    return `$cycle($sp(${head}, ${scale})${chain})`;
}
