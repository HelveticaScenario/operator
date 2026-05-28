/**
 * Migrate `$cycle("…")` calls to `$cycle($p("…"))` in DSL source.
 *
 * Three kinds of rewrites:
 * - Direct call sites: `$cycle(<literal>)` → `$cycle($p(<literal>))`
 * - Variable assignments: if a variable passed to `$cycle` is assigned only
 *   string literals in this source, wrap each assignment RHS instead.
 * - Comments: same shape, found by regex on the raw text.
 *
 * Idempotent — running on a migrated buffer is a no-op.
 * `$iCycle` is intentionally not touched.
 */

import { Node, Project } from 'ts-morph';
import type { CallExpression, Expression, SourceFile } from 'ts-morph';

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
        if (name !== '$cycle') return;
        const args = node.getArguments();
        if (args.length === 0) return;
        const first = args[0] as Expression;

        if (isAlreadyWrapped(first)) return;

        if (isStringish(first)) {
            edits.push(wrap(first.getStart(), first.getEnd(), first.getText()));
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
    });

    let assignmentsChanged = 0;
    for (const name of variablesToRewrite) {
        const sites = varAssignments.get(name);
        if (!sites) continue;
        for (const site of sites) {
            if (site.rhsText.trimStart().startsWith('$p(')) continue;
            edits.push(wrap(site.rhsStart, site.rhsEnd, site.rhsText));
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

function isAlreadyWrapped(node: Expression): boolean {
    if (!Node.isCallExpression(node)) return false;
    const expr = node.getExpression();
    return Node.isIdentifier(expr) && expr.getText() === '$p';
}

function wrap(start: number, end: number, text: string): Edit {
    return { start, end, replacement: `$p(${text})` };
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

function collectCommentEdits(source: string): {
    commentEdits: Edit[];
    commentsChanged: number;
} {
    const edits: Edit[] = [];
    let count = 0;

    for (const commentMatch of source.matchAll(COMMENT_RE)) {
        const commentText = commentMatch[0];
        const commentStart = commentMatch.index ?? 0;

        for (const inner of commentText.matchAll(CYCLE_IN_COMMENT_RE)) {
            const literal = inner[1];
            const literalRelStart =
                (inner.index ?? 0) + inner[0].indexOf(literal);
            const literalAbsStart = commentStart + literalRelStart;
            const literalAbsEnd = literalAbsStart + literal.length;

            // Guard against `$p($cycle("…"))` shapes inside comments — only
            // skip if a `$p(` immediately precedes the `$cycle` token.
            const cycleAbsStart = commentStart + (inner.index ?? 0);
            const before = source.slice(0, cycleAbsStart).trimEnd();
            if (before.endsWith('$p(')) continue;

            edits.push(wrap(literalAbsStart, literalAbsEnd, literal));
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
            // Overlapping edits — bail out and return original. Defensive;
            // the passes above shouldn't produce overlap.
            return source;
        }
        out += source.slice(cursor, edit.start);
        out += edit.replacement;
        cursor = edit.end;
    }
    out += source.slice(cursor);
    return out;
}
