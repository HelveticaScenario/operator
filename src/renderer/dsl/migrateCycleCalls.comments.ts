/**
 * Comment-body rewrites for `migrateCycleCalls`. Comments are matched by
 * regex against the raw source string (ts-morph doesn't expose them as
 * Expression nodes), then transformed by the same shape rules as live
 * code:
 *
 * - `$cycle("…")`            → `$cycle($p("…"))`
 * - `$iCycle("p", "s")`      → `$cycle($sp("p", "s"))`
 * - `$iCycle([s0, s1], "s")` → `$cycle($sp(s0, "s").add(s1)…)`
 */

import type { Edit } from './migrateCycleCalls.shared';
import { buildSpReplacement, wrapP } from './migrateCycleCalls.shared';

const COMMENT_RE = /(\/\/[^\n]*)|(\/\*[\s\S]*?\*\/)/g;
const CYCLE_IN_COMMENT_RE =
    /\$cycle\s*\(\s*("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)\s*\)/g;
// Single-string $iCycle in comments: $iCycle("…", "…").
const ICYCLE_STRING_IN_COMMENT_RE =
    /\$iCycle\s*\(\s*("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)\s*,\s*("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)\s*\)/g;
// Array-form $iCycle in comments: $iCycle([…], "…"). Captures the array
// body so we can split on string literals inside.
const ICYCLE_ARRAY_IN_COMMENT_RE =
    /\$iCycle\s*\(\s*\[([^\]]*)\]\s*,\s*("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)\s*\)/g;
// Single string literal inside an array body.
const ARRAY_STRING_RE =
    /("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)/g;

export function collectCommentEdits(source: string): {
    commentEdits: Edit[];
    commentsChanged: number;
} {
    const edits: Edit[] = [];
    let count = 0;

    for (const commentMatch of source.matchAll(COMMENT_RE)) {
        const commentText = commentMatch[0];
        const commentStart = commentMatch.index ?? 0;

        // 1. $cycle("…") → $cycle($p("…"))
        for (const inner of commentText.matchAll(CYCLE_IN_COMMENT_RE)) {
            const literal = inner[1];
            const literalRelStart =
                (inner.index ?? 0) + inner[0].indexOf(literal);
            const literalAbsStart = commentStart + literalRelStart;
            const literalAbsEnd = literalAbsStart + literal.length;

            const cycleAbsStart = commentStart + (inner.index ?? 0);
            const before = source.slice(0, cycleAbsStart).trimEnd();
            if (before.endsWith('$p(')) continue;

            edits.push(wrapP(literalAbsStart, literalAbsEnd, literal));
            count += 1;
        }

        // 2. $iCycle("p", "s") → $cycle($sp("p", "s"))
        for (const inner of commentText.matchAll(ICYCLE_STRING_IN_COMMENT_RE)) {
            const callAbsStart = commentStart + (inner.index ?? 0);
            const callAbsEnd = callAbsStart + inner[0].length;
            const replacement = buildSpReplacement([inner[1]], inner[2]);
            edits.push({
                start: callAbsStart,
                end: callAbsEnd,
                replacement,
            });
            count += 1;
        }

        // 3. $iCycle([s0, s1, …], scale) → $cycle($sp(s0, scale).add(s1)…)
        for (const inner of commentText.matchAll(ICYCLE_ARRAY_IN_COMMENT_RE)) {
            const callAbsStart = commentStart + (inner.index ?? 0);
            const callAbsEnd = callAbsStart + inner[0].length;
            const arrayBody = inner[1];
            const scaleLit = inner[2];
            const sources = Array.from(
                arrayBody.matchAll(ARRAY_STRING_RE),
                (m) => m[1],
            );
            if (sources.length === 0) continue;
            const replacement = buildSpReplacement(sources, scaleLit);
            edits.push({
                start: callAbsStart,
                end: callAbsEnd,
                replacement,
            });
            count += 1;
        }
    }

    return { commentEdits: edits, commentsChanged: count };
}
