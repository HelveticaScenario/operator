/**
 * Migrate legacy `$cycle("…")` and `$iCycle(…, scale)` calls to the
 * new pattern API in DSL source.
 *
 * `$cycle("…")`  → `$cycle($p("…"))`
 * `$iCycle(src, scale)` → `$cycle($sp(src, scale))`
 * `$iCycle([s0, s1, s2], scale)` → `$cycle($sp(s0, scale).add(s1).add(s2))`
 *
 * Three kinds of rewrites for each:
 * - Direct call sites: wrap / transform the literal arguments.
 * - Variable assignments: if a variable passed to `$cycle` is assigned
 *   only string literals in this source, wrap each assignment RHS
 *   instead of the call site.
 * - Comments: same shapes, found by regex on the raw text.
 *
 * Idempotent — running on a migrated buffer is a no-op.
 */

import { Node, Project } from 'ts-morph';
import type {
    ArrayLiteralExpression,
    CallExpression,
    Expression,
    SourceFile,
} from 'ts-morph';

export interface MigrationResult {
    migrated: string;
    callsChanged: number;
    assignmentsChanged: number;
    commentsChanged: number;
    skippedVariables: string[];
    error?: string;
}

interface Edit {
    start: number;
    end: number;
    replacement: string;
}

interface AssignmentSite {
    rhsStart: number;
    rhsEnd: number;
    rhsText: string;
    kind: 'string' | 'non-string';
}

const ZERO_COUNTS = {
    callsChanged: 0,
    assignmentsChanged: 0,
    commentsChanged: 0,
} as const;

export function migrateCycleCalls(source: string): MigrationResult {
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
            skippedVariables: [],
            error: err instanceof Error ? err.message : String(err),
        };
    }

    const varAssignments = collectAssignments(sourceFile);
    const edits: Edit[] = [];
    const skippedVariables = new Set<string>();
    const variablesToRewrite = new Set<string>();
    let callsChanged = 0;

    sourceFile.forEachDescendant((node) => {
        if (!Node.isCallExpression(node)) return;
        const name = getCalledName(node);

        if (name === '$cycle') {
            const args = node.getArguments();
            if (args.length === 0) return;
            const first = args[0] as Expression;
            if (isPWrapped(first)) return;
            if (isStringish(first)) {
                edits.push(
                    wrapP(first.getStart(), first.getEnd(), first.getText()),
                );
                callsChanged += 1;
                return;
            }
            if (Node.isIdentifier(first)) {
                const varName = first.getText();
                const sites = varAssignments.get(varName);
                if (!sites || sites.length === 0) return;
                if (sites.some((s) => s.kind === 'non-string')) {
                    skippedVariables.add(varName);
                    return;
                }
                variablesToRewrite.add(varName);
            }
            return;
        }

        if (name === '$iCycle') {
            const args = node.getArguments();
            if (args.length < 2) return;
            const patternsArg = args[0] as Expression;
            const scaleArg = args[1] as Expression;

            const sources = resolveISources(patternsArg, varAssignments);
            const scale = resolveIScale(scaleArg, varAssignments);
            if (!sources || !scale) {
                if (Node.isIdentifier(patternsArg)) {
                    skippedVariables.add(patternsArg.getText());
                }
                if (Node.isIdentifier(scaleArg)) {
                    skippedVariables.add(scaleArg.getText());
                }
                return;
            }

            const replacement = buildSpReplacement(sources, scale);
            edits.push({
                start: node.getStart(),
                end: node.getEnd(),
                replacement,
            });
            callsChanged += 1;
        }
    });

    let assignmentsChanged = 0;
    for (const name of variablesToRewrite) {
        const sites = varAssignments.get(name);
        if (!sites) continue;
        for (const site of sites) {
            if (site.rhsText.trimStart().startsWith('$p(')) continue;
            edits.push(wrapP(site.rhsStart, site.rhsEnd, site.rhsText));
            assignmentsChanged += 1;
        }
    }

    const { commentEdits, commentsChanged } = collectCommentEdits(source);
    edits.push(...commentEdits);

    const migrated = applyEdits(source, edits);

    return {
        migrated,
        callsChanged,
        assignmentsChanged,
        commentsChanged,
        skippedVariables: Array.from(skippedVariables).sort(),
    };
}

function getCalledName(call: CallExpression): string | null {
    const expr = call.getExpression();
    if (Node.isIdentifier(expr)) return expr.getText();
    if (Node.isPropertyAccessExpression(expr)) return expr.getName();
    return null;
}

function isStringish(node: Expression): boolean {
    return (
        Node.isStringLiteral(node) ||
        Node.isNoSubstitutionTemplateLiteral(node) ||
        Node.isTemplateExpression(node)
    );
}

function isPWrapped(node: Expression): boolean {
    if (!Node.isCallExpression(node)) return false;
    const expr = node.getExpression();
    return Node.isIdentifier(expr) && expr.getText() === '$p';
}

function wrapP(start: number, end: number, text: string): Edit {
    return { start, end, replacement: `$p(${text})` };
}

/**
 * Resolve the patterns arg of `$iCycle` into an ordered list of source
 * literals (verbatim text including quotes). Returns null if the shape
 * can't be reduced statically.
 */
function resolveISources(
    arg: Expression,
    assignments: Map<string, AssignmentSite[]>,
): string[] | null {
    if (isStringish(arg)) return [arg.getText()];
    if (Node.isArrayLiteralExpression(arg)) {
        return collectArrayStrings(arg);
    }
    if (Node.isIdentifier(arg)) {
        const sites = assignments.get(arg.getText());
        if (!sites || sites.length !== 1) return null;
        const only = sites[0];
        if (only.kind === 'string') return [only.rhsText];
        // Find the original declaration to check whether it's an array
        // of string literals — array RHS texts are kind 'non-string' in
        // the site cache but still mechanically expandable.
        const decl = arg
            .getSourceFile()
            .forEachDescendantAsArray()
            .find(
                (n) =>
                    Node.isVariableDeclaration(n) &&
                    n.getName() === arg.getText(),
            );
        if (decl && Node.isVariableDeclaration(decl)) {
            const init = decl.getInitializer();
            if (init && Node.isArrayLiteralExpression(init)) {
                return collectArrayStrings(init);
            }
        }
        return null;
    }
    return null;
}

function collectArrayStrings(
    arr: ArrayLiteralExpression,
): string[] | null {
    const out: string[] = [];
    for (const elem of arr.getElements()) {
        if (isStringish(elem)) {
            out.push(elem.getText());
            continue;
        }
        return null;
    }
    return out.length > 0 ? out : null;
}

function resolveIScale(
    arg: Expression,
    assignments: Map<string, AssignmentSite[]>,
): string | null {
    if (isStringish(arg)) return arg.getText();
    if (Node.isIdentifier(arg)) {
        // Preserve identifier reference verbatim — don't inline the
        // variable's value. The $sp call site should still read `scale`
        // from the same binding the caller declared.
        const sites = assignments.get(arg.getText());
        if (!sites || sites.length !== 1) return null;
        if (sites[0].kind !== 'string') return null;
        return arg.getText();
    }
    return null;
}

function buildSpReplacement(sources: string[], scale: string): string {
    const [head, ...rest] = sources;
    const chain = rest.map((rhs) => `.add(${rhs})`).join('');
    return `$cycle($sp(${head}, ${scale})${chain})`;
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
            push(map, nameNode.getText(), siteFromExpression(init));
            return;
        }

        if (Node.isBinaryExpression(node)) {
            if (node.getOperatorToken().getText() !== '=') return;
            const left = node.getLeft();
            if (!Node.isIdentifier(left)) return;
            push(map, left.getText(), siteFromExpression(node.getRight()));
        }
    });

    return map;
}

function siteFromExpression(expr: Expression): AssignmentSite {
    return {
        rhsStart: expr.getStart(),
        rhsEnd: expr.getEnd(),
        rhsText: expr.getText(),
        kind: isStringish(expr) ? 'string' : 'non-string',
    };
}

function push(
    map: Map<string, AssignmentSite[]>,
    name: string,
    site: AssignmentSite,
): void {
    const list = map.get(name);
    if (list) {
        list.push(site);
    } else {
        map.set(name, [site]);
    }
}

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

function collectCommentEdits(source: string): {
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

function applyEdits(source: string, edits: Edit[]): string {
    if (edits.length === 0) return source;
    const sorted = [...edits].sort((a, b) => a.start - b.start);
    let out = '';
    let cursor = 0;
    for (const edit of sorted) {
        if (edit.start < cursor) {
            return source;
        }
        out += source.slice(cursor, edit.start);
        out += edit.replacement;
        cursor = edit.end;
    }
    out += source.slice(cursor);
    return out;
}

