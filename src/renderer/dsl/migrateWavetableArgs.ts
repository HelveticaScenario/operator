/**
 * Migrate legacy `$wavetable(wav, pitch, …)` calls to the new pitch-first
 * argument order `$wavetable(pitch, wav, …)`.
 *
 * The wavetable oscillator used to take its wavetable (a `$wavs()` handle)
 * as the first argument and the pitch as the second. The order is now
 * pitch-first, matching every other oscillator. This rewrites old call sites
 * by swapping the first two arguments; the third argument (`position`) and
 * the trailing config object keep their places.
 *
 * Detection follows the user-facing rule: a call is legacy when its SECOND
 * argument is a `Poly<Signal>` (a pitch) rather than a wavetable handle.
 * Because the only two things that can occupy those slots are a wavetable
 * handle (`Wav`) or a pitch (`Poly<Signal>`), "the second arg is a
 * Poly<Signal>" is the same as "the second arg is not a wavetable handle".
 *
 * A wavetable handle is recognized syntactically: a `$wavs()…` member/element
 * chain, or an identifier bound (in this source, in every visible assignment)
 * to such a chain. To stay safe against already-migrated code whose wav arg
 * we cannot resolve (e.g. one returned from a helper call), a swap also
 * requires the FIRST argument to be a recognizable wavetable handle — a
 * genuine legacy call always has the wav first. Calls that look legacy but
 * whose wav arg can't be identified are reported for manual review rather
 * than rewritten.
 *
 * Two kinds of rewrites:
 * - Direct call sites: swap the first two argument spans.
 * - Comments: same shape, found by scanning comment text.
 *
 * Idempotent — a call already in `(pitch, wav)` order (its second argument
 * IS a wavetable handle) is left untouched.
 */

import { Node, Project } from 'ts-morph';
import type { CallExpression, Expression, SourceFile } from 'ts-morph';

import type { Edit } from './migrationEdits';
import { applyEdits } from './migrationEdits';
import type { MigrationMeta } from './migrations/types';

export interface WavetableMigrationResult {
    migrated: string;
    callsChanged: number;
    commentsChanged: number;
    /** Human-readable descriptions of calls that look legacy but could not be
     *  rewritten safely, so the user can fix them by hand. */
    skipped: string[];
    error?: string;
}

const ZERO_COUNTS = { callsChanged: 0, commentsChanged: 0 } as const;

/** An assignment to a name: the RHS expression plus the declared/assigned
 *  identifier node (used to confirm a reference binds to the same symbol). */
interface AssignmentSite {
    rhs: Expression;
    nameNode: Node;
}

export function migrateWavetableArgs(source: string): WavetableMigrationResult {
    let sourceFile: SourceFile;
    try {
        const project = new Project({
            compilerOptions: { allowJs: true, checkJs: false, noEmit: true },
            useInMemoryFileSystem: true,
        });
        sourceFile = project.createSourceFile('migrate.ts', source);
    } catch (err) {
        return {
            migrated: source,
            ...ZERO_COUNTS,
            skipped: [],
            error: err instanceof Error ? err.message : String(err),
        };
    }

    const assignments = collectAssignments(sourceFile);
    const edits: Edit[] = [];
    const skipped: string[] = [];
    let callsChanged = 0;

    sourceFile.forEachDescendant((node) => {
        if (!Node.isCallExpression(node)) return;
        if (getCalledName(node) !== '$wavetable') return;

        const args = node.getArguments();
        if (args.length === 0) return;

        const arg0 = args[0] as Expression;
        const arg1 = args.length >= 2 ? (args[1] as Expression) : undefined;

        const wav0 = isWavHandle(arg0, assignments);
        const wav1 = arg1 ? isWavHandle(arg1, assignments) : false;

        // Second arg is already a wavetable handle → new order → no-op.
        if (wav1) return;

        // First arg is the wav, second is a pitch → legacy → swap.
        if (arg1 && wav0) {
            edits.push({
                start: arg0.getStart(),
                end: arg0.getEnd(),
                replacement: arg1.getText(),
            });
            edits.push({
                start: arg1.getStart(),
                end: arg1.getEnd(),
                replacement: arg0.getText(),
            });
            callsChanged += 1;
            return;
        }

        // Looks like it might be legacy, but we can't identify the wav arg
        // (a single wav-only call, or neither arg resolves to a `$wavs()`
        // handle). Don't guess — flag it for manual review.
        if ((arg1 && !wav0) || (!arg1 && wav0)) {
            skipped.push(describeCall(node));
        }
    });

    const { commentEdits, commentsChanged } = collectCommentEdits(source);
    edits.push(...commentEdits);

    const { source: migrated, conflict } = applyEdits(source, edits);
    if (conflict) {
        return {
            migrated: source,
            ...ZERO_COUNTS,
            skipped: [],
            error: 'edits conflict',
        };
    }

    return {
        migrated,
        callsChanged,
        commentsChanged,
        skipped: dedupe(skipped),
    };
}

function getCalledName(call: CallExpression): string | null {
    const expr = call.getExpression();
    if (Node.isIdentifier(expr)) return expr.getText();
    if (Node.isPropertyAccessExpression(expr)) return expr.getName();
    return null;
}

function describeCall(call: CallExpression): string {
    const line = call.getStartLineNumber();
    const text = call.getText().replace(/\s+/g, ' ');
    const short = text.length > 60 ? `${text.slice(0, 57)}…` : text;
    return `line ${line}: ${short}`;
}

function dedupe(items: string[]): string[] {
    return Array.from(new Set(items));
}

/**
 * A wavetable handle is a `$wavs()`-rooted member/element chain, or an
 * identifier bound to one in every visible assignment in this source.
 */
function isWavHandle(
    node: Expression,
    assignments: Map<string, AssignmentSite[]>,
): boolean {
    if (isWavsRooted(node)) return true;
    if (Node.isIdentifier(node)) {
        const sites = assignments.get(node.getText());
        if (!sites || sites.length === 0) return false;
        if (!assignmentsVisibleAt(sites, node)) return false;
        return sites.every((s) => isWavsRooted(s.rhs));
    }
    return false;
}

/**
 * Walk a member/element-access chain down to its root and report whether that
 * root is a `$wavs(...)` call — i.e. the expression reads a handle out of the
 * loaded WAV tree (`$wavs().pad`, `$wavs().tables.warm`, `$wavs()[0]`).
 */
function isWavsRooted(node: Node): boolean {
    let current: Node = node;
    while (
        Node.isPropertyAccessExpression(current) ||
        Node.isElementAccessExpression(current) ||
        Node.isNonNullExpression(current) ||
        Node.isParenthesizedExpression(current)
    ) {
        const inner = current.getExpression();
        if (!inner) return false;
        current = inner;
    }
    if (!Node.isCallExpression(current)) return false;
    const callee = current.getExpression();
    return Node.isIdentifier(callee) && callee.getText() === '$wavs';
}

function collectAssignments(
    sourceFile: SourceFile,
): Map<string, AssignmentSite[]> {
    const map = new Map<string, AssignmentSite[]>();

    sourceFile.forEachDescendant((node) => {
        if (Node.isVariableDeclaration(node)) {
            const nameNode = node.getNameNode();
            if (!Node.isIdentifier(nameNode)) return;
            const init = node.getInitializer();
            if (!init) return;
            push(map, nameNode.getText(), { rhs: init, nameNode });
            return;
        }
        if (Node.isBinaryExpression(node)) {
            if (node.getOperatorToken().getText() !== '=') return;
            const left = node.getLeft();
            if (!Node.isIdentifier(left)) return;
            push(map, left.getText(), { rhs: node.getRight(), nameNode: left });
        }
    });

    return map;
}

/**
 * `assignments` is keyed by name only, so a name shadowed by a different
 * binding would mix sites from unrelated declarations. Resolve the reference
 * to its declaration symbol and require every site to bind to that same
 * symbol; otherwise the name is ambiguous and must not drive a rewrite.
 */
function assignmentsVisibleAt(
    sites: AssignmentSite[],
    reference: Node,
): boolean {
    const refSym = reference.getSymbol()?.compilerSymbol;
    if (!refSym) return false;
    return sites.every(
        (s) => s.nameNode.getSymbol()?.compilerSymbol === refSym,
    );
}

function push(
    map: Map<string, AssignmentSite[]>,
    name: string,
    site: AssignmentSite,
): void {
    const list = map.get(name);
    if (list) list.push(site);
    else map.set(name, [site]);
}

// ─── Comments ────────────────────────────────────────────────────────────

/**
 * Extract comment spans from `source`, skipping string literals so that
 * comment-like text inside a string (a `"…// …"` doc string, an `https://`
 * URL) is never mistaken for a comment. Single-pass: at each position a string
 * literal is consumed whole — via {@link skipString} — before `//` or `/*` can
 * open a comment.
 */
function scanComments(source: string): { start: number; text: string }[] {
    const comments: { start: number; text: string }[] = [];
    const n = source.length;
    let i = 0;
    while (i < n) {
        const strEnd = skipString(source, i);
        if (strEnd !== -1) {
            i = strEnd;
            continue;
        }
        if (source[i] === '/' && source[i + 1] === '/') {
            let j = i + 2;
            while (j < n && source[j] !== '\n') j += 1;
            comments.push({ start: i, text: source.slice(i, j) });
            i = j;
            continue;
        }
        if (source[i] === '/' && source[i + 1] === '*') {
            let j = i + 2;
            while (j < n && !(source[j] === '*' && source[j + 1] === '/')) {
                j += 1;
            }
            const end = j < n ? j + 2 : n;
            comments.push({ start: i, text: source.slice(i, end) });
            i = end;
            continue;
        }
        i += 1;
    }
    return comments;
}

/**
 * Swap `$wavetable(wav, pitch, …)` → `$wavetable(pitch, wav, …)` inside
 * comments. ts-morph doesn't expose comments as expression nodes, so they're
 * scanned as raw text (string literals excluded): each `$wavetable(` is matched
 * to its closing paren (respecting nested brackets and string literals), its
 * top-level arguments are split, and the first two are swapped when the first
 * is a `$wavs()` handle and the second is not.
 */
function collectCommentEdits(source: string): {
    commentEdits: Edit[];
    commentsChanged: number;
} {
    const edits: Edit[] = [];
    let count = 0;

    for (const comment of scanComments(source)) {
        const commentText = comment.text;
        const commentStart = comment.start;

        for (const call of scanCalls(commentText, '$wavetable')) {
            const argSpans = splitTopLevelArgs(
                commentText,
                call.argsStart,
                call.argsEnd,
            );
            if (!argSpans || argSpans.length < 2) continue;

            const a0 = argSpans[0];
            const a1 = argSpans[1];
            const t0 = commentText.slice(a0.start, a0.end);
            const t1 = commentText.slice(a1.start, a1.end);
            if (!isWavsRootedText(t0) || isWavsRootedText(t1)) continue;

            edits.push({
                start: commentStart + a0.start,
                end: commentStart + a0.end,
                replacement: t1,
            });
            edits.push({
                start: commentStart + a1.start,
                end: commentStart + a1.end,
                replacement: t0,
            });
            count += 1;
        }
    }

    return { commentEdits: edits, commentsChanged: count };
}

/** Text-only counterpart of {@link isWavsRooted}: the argument text begins
 *  with a `$wavs(` call (variables can't be resolved inside comments). */
function isWavsRootedText(text: string): boolean {
    return /^\$wavs\s*\(/.test(text.trim());
}

interface CallSpan {
    /** Offset just after the call's opening paren. */
    argsStart: number;
    /** Offset of the call's closing paren. */
    argsEnd: number;
}

/**
 * Find top-level `fnName(...)` calls in `text`, returning each call's
 * argument span. The opening paren is matched to its closing paren while
 * tracking `()[]{}` depth and skipping string literals, so brackets or
 * commas inside arguments don't end the call early. Calls whose parens never
 * balance (truncated text) are dropped.
 */
function scanCalls(text: string, fnName: string): CallSpan[] {
    const spans: CallSpan[] = [];
    const callRe = new RegExp(`${escapeRegExp(fnName)}\\s*\\(`, 'g');
    for (const m of text.matchAll(callRe)) {
        const matchStart = m.index ?? 0;
        // Reject matches where fnName is the tail of a longer identifier.
        const prev = text[matchStart - 1];
        if (prev && /[\w$]/.test(prev)) continue;

        const openParen = matchStart + m[0].length - 1;
        const closeParen = matchParen(text, openParen);
        if (closeParen === -1) continue;
        spans.push({ argsStart: openParen + 1, argsEnd: closeParen });
    }
    return spans;
}

/** Index of the `)` matching the `(` at `open`, or -1 if unbalanced. */
function matchParen(text: string, open: number): number {
    let depth = 0;
    let i = open;
    while (i < text.length) {
        const ch = text[i];
        const strEnd = skipString(text, i);
        if (strEnd !== -1) {
            i = strEnd;
            continue;
        }
        if (ch === '(' || ch === '[' || ch === '{') depth += 1;
        else if (ch === ')' || ch === ']' || ch === '}') {
            depth -= 1;
            if (depth === 0) return ch === ')' ? i : -1;
        }
        i += 1;
    }
    return -1;
}

/**
 * Split the argument list spanning `[start, end)` of `text` into top-level
 * arguments, returning each one's trimmed `[start, end)` span. Commas inside
 * nested brackets or string literals are ignored. Returns null if a bracket
 * never closes within the span.
 */
function splitTopLevelArgs(
    text: string,
    start: number,
    end: number,
): { start: number; end: number }[] | null {
    const spans: { start: number; end: number }[] = [];
    let depth = 0;
    let segStart = start;
    let i = start;
    while (i < end) {
        const ch = text[i];
        const strEnd = skipString(text, i);
        if (strEnd !== -1) {
            i = Math.min(strEnd, end);
            continue;
        }
        if (ch === '(' || ch === '[' || ch === '{') depth += 1;
        else if (ch === ')' || ch === ']' || ch === '}') {
            depth -= 1;
            if (depth < 0) return null;
        } else if (ch === ',' && depth === 0) {
            spans.push(trimSpan(text, segStart, i));
            segStart = i + 1;
        }
        i += 1;
    }
    if (depth !== 0) return null;
    const tail = trimSpan(text, segStart, end);
    // Drop a trailing empty segment from `f(a, )` but keep a lone empty `f()`.
    if (tail.start < tail.end || spans.length === 0) spans.push(tail);
    return spans;
}

/** If `text[i]` opens a string literal, return the index just past its close;
 *  otherwise -1. Handles `'`, `"`, and backtick delimiters with `\` escapes. */
function skipString(text: string, i: number): number {
    const quote = text[i];
    if (quote !== "'" && quote !== '"' && quote !== '`') return -1;
    let j = i + 1;
    while (j < text.length) {
        const ch = text[j];
        if (ch === '\\') {
            j += 2;
            continue;
        }
        if (ch === quote) return j + 1;
        j += 1;
    }
    return text.length;
}

function trimSpan(
    text: string,
    start: number,
    end: number,
): { start: number; end: number } {
    let s = start;
    let e = end;
    while (s < e && /\s/.test(text[s])) s += 1;
    while (e > s && /\s/.test(text[e - 1])) e -= 1;
    return { start: s, end: e };
}

function escapeRegExp(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Registry entry: shipped in v0.0.97. */
export const meta: MigrationMeta = {
    id: 'wavetable-pitch-first',
    sinceVersion: '0.0.97',
    order: 2,
    title: 'Migrate $wavetable to pitch-first order',
    skippedLabel: 'Needs manual review:',
    run(source) {
        const result = migrateWavetableArgs(source);
        return {
            migrated: result.migrated,
            changed: result.migrated !== source,
            summary: {
                callsChanged: result.callsChanged,
                commentsChanged: result.commentsChanged,
                skippedVariables: result.skipped,
                error: result.error,
            },
        };
    },
};
