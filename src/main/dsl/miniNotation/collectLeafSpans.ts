/**
 * Collect all leaf source spans from a MiniAST.
 *
 * Direct TypeScript port of Rust's `collect_leaf_spans` in
 * `crates/modular_core/src/pattern_system/mini/ast.rs`. Used by `$p()` to
 * pre-compute the `all_spans` field so Monaco can create tracked decorations
 * without round-tripping through the Rust side.
 *
 * The four AST types (`MiniAST`, `MiniASTF64`, `MiniASTU32`, `MiniASTI32`)
 * share an identical recursive structure — only leaf value types and
 * modifier-child types differ. `walk` handles the common shape; only
 * `Fast`, `Slow`, and `Euclidean` need per-type dispatch to route into
 * the correct sibling walker for nested modifier args.
 */

import type {
    MiniAST,
    MiniASTF64,
    MiniASTI32,
    MiniASTU32,
    SourceSpan,
} from './ast';

type Span = [number, number];

type AnyAst = MiniAST | MiniASTF64 | MiniASTU32 | MiniASTI32;

function pushSpan(spans: Span[], span: SourceSpan): void {
    spans.push([span.start, span.end]);
}

/**
 * Recursively walk an AST, collecting leaf spans. The `self` callback
 * is the current type's own walker (used for recursing into same-type
 * children). `Fast`/`Slow`/`Euclidean` modifier arguments are handled
 * by the caller before delegating here.
 */
function walkCommon<T extends AnyAst>(
    ast: T,
    out: Span[],
    self: (child: T, out: Span[]) => void,
): 'handled' | 'modifier' {
    if ('Pure' in ast) {
        pushSpan(out, ast.Pure.span);
        return 'handled';
    }
    if ('Rest' in ast) {
        pushSpan(out, ast.Rest);
        return 'handled';
    }
    if ('List' in ast) {
        for (const child of ast.List.node as T[]) self(child, out);
        return 'handled';
    }
    if ('Sequence' in ast) {
        for (const [child] of ast.Sequence as Array<[T, number | null]>)
            self(child, out);
        return 'handled';
    }
    if ('FastCat' in ast) {
        for (const [child] of ast.FastCat as Array<[T, number | null]>)
            self(child, out);
        return 'handled';
    }
    if ('SlowCat' in ast) {
        for (const [child] of ast.SlowCat as Array<[T, number | null]>)
            self(child, out);
        return 'handled';
    }
    if ('RandomChoice' in ast) {
        const [children] = ast.RandomChoice as [T[], number];
        for (const child of children) self(child, out);
        return 'handled';
    }
    if ('Stack' in ast) {
        for (const child of ast.Stack as T[]) self(child, out);
        return 'handled';
    }
    if ('Replicate' in ast) {
        const [pattern] = ast.Replicate as [T, number];
        self(pattern, out);
        return 'handled';
    }
    if ('Degrade' in ast) {
        const [pattern] = ast.Degrade as [T, number | null, number];
        self(pattern, out);
        return 'handled';
    }
    // Fast / Slow / Euclidean — caller handles modifier-arg routing.
    return 'modifier';
}

/** Collect leaf spans from a top-level `MiniAST`. */
export function collectLeafSpans(ast: MiniAST): Span[] {
    const out: Span[] = [];
    walkMini(ast, out);
    return out;
}

function walkMini(ast: MiniAST, out: Span[]): void {
    if (walkCommon(ast, out, walkMini) === 'handled') return;
    if ('Fast' in ast) {
        const [pattern, factor] = ast.Fast;
        walkMini(pattern, out);
        walkF64(factor, out);
        return;
    }
    if ('Slow' in ast) {
        const [pattern, factor] = ast.Slow;
        walkMini(pattern, out);
        // Slow's factor is MiniAST, not MiniASTF64 (matches the Rust type).
        walkMini(factor, out);
        return;
    }
    if ('Euclidean' in ast) {
        const { pattern, pulses, steps, rotation } = ast.Euclidean;
        walkMini(pattern, out);
        walkU32(pulses, out);
        walkU32(steps, out);
        if (rotation) walkI32(rotation, out);
        return;
    }
    if ('Polymeter' in ast) {
        const { children, steps_per_cycle } = ast.Polymeter;
        for (const child of children) walkMini(child, out);
        if (steps_per_cycle) walkF64(steps_per_cycle, out);
    }
}

function walkF64(ast: MiniASTF64, out: Span[]): void {
    if (walkCommon(ast, out, walkF64) === 'handled') return;
    if ('Fast' in ast) {
        const [pattern, factor] = ast.Fast;
        walkF64(pattern, out);
        walkF64(factor, out);
        return;
    }
    if ('Slow' in ast) {
        const [pattern, factor] = ast.Slow;
        walkF64(pattern, out);
        walkF64(factor, out);
        return;
    }
    if ('Euclidean' in ast) {
        const { pattern, pulses, steps, rotation } = ast.Euclidean;
        walkF64(pattern, out);
        walkU32(pulses, out);
        walkU32(steps, out);
        if (rotation) walkI32(rotation, out);
        return;
    }
    if ('Polymeter' in ast) {
        const { children, steps_per_cycle } = ast.Polymeter;
        for (const child of children) walkF64(child, out);
        if (steps_per_cycle) walkF64(steps_per_cycle, out);
    }
}

function walkU32(ast: MiniASTU32, out: Span[]): void {
    if (walkCommon(ast, out, walkU32) === 'handled') return;
    if ('Fast' in ast) {
        const [pattern, factor] = ast.Fast;
        walkU32(pattern, out);
        walkF64(factor, out);
        return;
    }
    if ('Slow' in ast) {
        const [pattern, factor] = ast.Slow;
        walkU32(pattern, out);
        walkF64(factor, out);
        return;
    }
    if ('Euclidean' in ast) {
        const { pattern, pulses, steps, rotation } = ast.Euclidean;
        walkU32(pattern, out);
        walkU32(pulses, out);
        walkU32(steps, out);
        if (rotation) walkI32(rotation, out);
        return;
    }
    if ('Polymeter' in ast) {
        const { children, steps_per_cycle } = ast.Polymeter;
        for (const child of children) walkU32(child, out);
        if (steps_per_cycle) walkF64(steps_per_cycle, out);
    }
}

function walkI32(ast: MiniASTI32, out: Span[]): void {
    if (walkCommon(ast, out, walkI32) === 'handled') return;
    if ('Fast' in ast) {
        const [pattern, factor] = ast.Fast;
        walkI32(pattern, out);
        walkF64(factor, out);
        return;
    }
    if ('Slow' in ast) {
        const [pattern, factor] = ast.Slow;
        walkI32(pattern, out);
        walkF64(factor, out);
        return;
    }
    if ('Euclidean' in ast) {
        const { pattern, pulses, steps, rotation } = ast.Euclidean;
        walkI32(pattern, out);
        walkU32(pulses, out);
        walkU32(steps, out);
        if (rotation) walkI32(rotation, out);
        return;
    }
    if ('Polymeter' in ast) {
        const { children, steps_per_cycle } = ast.Polymeter;
        for (const child of children) walkI32(child, out);
        if (steps_per_cycle) walkF64(steps_per_cycle, out);
    }
}
