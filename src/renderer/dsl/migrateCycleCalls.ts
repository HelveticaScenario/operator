/**
 * Migrate legacy `$cycle("…")` and `$iCycle(…, scale)` calls to the
 * new pattern API in DSL source.
 *
 * `$cycle("…")`  → `$cycle($p("…"))`
 * `$iCycle(src, scale)` → `$cycle($p.s(src, scale))`
 * `$iCycle([s0, s1, s2], scale)` → `$cycle($p.s(s0, scale).add(s1).add(s2))`
 *
 * When the `$iCycle` source is a string-valued variable, the `$p.s(…, scale)`
 * wrap is pushed into the variable's assignments instead, and the call
 * collapses to `$cycle(var)` — mirroring how `$cycle("…")` wraps assignments
 * with `$p`:
 *   `let pat = '…'; pat = '…'; $iCycle(pat, key)`
 *   → `let pat = $p.s('…', key); pat = $p.s('…', key); $cycle(pat)`
 *
 * While wrapping a `$cycle` pattern, legacy voltage atoms lose their `v`
 * suffix (`$cycle("5v 3v")` → `$cycle($p("5 3"))`).
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

import { collectCommentEdits } from './migrateCycleCalls.comments';
import type { Edit } from './migrateCycleCalls.shared';
import { buildSpReplacement, wrapP } from './migrateCycleCalls.shared';
import { applyEdits } from './migrationEdits';

export interface MigrationResult {
    migrated: string;
    callsChanged: number;
    assignmentsChanged: number;
    commentsChanged: number;
    skippedVariables: string[];
    error?: string;
}

interface AssignmentSite {
    rhsStart: number;
    rhsEnd: number;
    rhsText: string;
    kind: 'string' | 'non-string';
    /** Identifier node being declared/assigned — resolves the binding so a
     *  rewrite never touches a different declaration that shares the name. */
    nameNode: Node;
    /** Original RHS expression node (kept for downstream array inspection). */
    initializerNode: Expression;
}

/** An `$iCycle(var, scale)` call whose source is a string-valued variable. */
interface SpVarCall {
    node: CallExpression;
    varName: string;
    scaleText: string;
}

/** A leading `$p(` / `$p.s(` marks an already-migrated pattern expression. */
function isPatternExpr(text: string): boolean {
    return /^\$(?:p|sp)\b/.test(text.trimStart());
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
    const spVarCalls: SpVarCall[] = [];
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
                if (!assignmentsVisibleAt(sites, first)) return;
                if (sites.some((s) => s.kind === 'non-string')) {
                    // Already-migrated pattern variables ($p/$p.s) are the
                    // finished form, not "skipped"; only flag genuinely
                    // unmigratable assignments.
                    if (!sites.every((s) => isPatternExpr(s.rhsText))) {
                        skippedVariables.add(varName);
                    }
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
            const scale = resolveIScale(scaleArg, varAssignments);

            // String-valued source variable → push `$p.s(…, scale)` into its
            // assignments and collapse the call to `$cycle(var)`. Deferred to
            // a post-pass so every call on the variable can agree on a scale.
            if (scale && Node.isIdentifier(patternsArg)) {
                const varName = patternsArg.getText();
                const sites = varAssignments.get(varName);
                if (
                    sites &&
                    sites.length > 0 &&
                    assignmentsVisibleAt(sites, patternsArg) &&
                    // Raw strings still need wrapping; already-`$p.s`-wrapped
                    // sites are the finished form (skipped in the push-down
                    // loop). A half-migrated variable mixing the two completes
                    // rather than being reported as unmigratable.
                    sites.every(
                        (s) => s.kind === 'string' || isPatternExpr(s.rhsText),
                    )
                ) {
                    spVarCalls.push({ node, varName, scaleText: scale });
                    return;
                }
            }

            const sources = resolveISources(patternsArg, varAssignments);
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

    // Resolve $iCycle string-variable sources collected above. When every
    // call on a variable agrees on the scale, push `$p.s(…, scale)` into the
    // variable's assignments and collapse each call to `$cycle(var)`. On a
    // scale disagreement, keep the variable as a raw string and inline `$p.s`
    // at each call site instead.
    const spByVar = new Map<string, SpVarCall[]>();
    for (const call of spVarCalls) {
        const list = spByVar.get(call.varName);
        if (list) list.push(call);
        else spByVar.set(call.varName, [call]);
    }
    for (const [varName, calls] of spByVar) {
        // A variable that also feeds a $cycle wants $p-wrapped assignments,
        // which is incompatible with $p.s-wrapping — leave both untouched.
        if (variablesToRewrite.has(varName)) {
            variablesToRewrite.delete(varName);
            skippedVariables.add(varName);
            continue;
        }
        const distinctScales = new Set(calls.map((c) => c.scaleText));
        if (distinctScales.size === 1) {
            const scale = calls[0].scaleText;
            const sites = varAssignments.get(varName);
            if (sites) {
                for (const site of sites) {
                    if (isPatternExpr(site.rhsText)) continue;
                    edits.push({
                        start: site.rhsStart,
                        end: site.rhsEnd,
                        replacement: `$p.s(${site.rhsText}, ${scale})`,
                    });
                    assignmentsChanged += 1;
                }
            }
            for (const call of calls) {
                edits.push({
                    start: call.node.getStart(),
                    end: call.node.getEnd(),
                    replacement: `$cycle(${varName})`,
                });
                callsChanged += 1;
            }
        } else {
            for (const call of calls) {
                edits.push({
                    start: call.node.getStart(),
                    end: call.node.getEnd(),
                    replacement: `$cycle($p.s(${varName}, ${call.scaleText}))`,
                });
                callsChanged += 1;
            }
        }
    }

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

    const { source: migrated, conflict } = applyEdits(source, edits);

    if (conflict) {
        return {
            migrated: source,
            ...ZERO_COUNTS,
            skippedVariables: [],
            error: 'edits conflict',
        };
    }

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

/**
 * Resolve a non-variable `$iCycle` patterns arg into an ordered list of
 * source texts for the `$p.s(...)` call. String literals and array literals
 * are reduced to verbatim text; a variable bound to a single array literal
 * expands to its elements. String-valued variables are handled separately
 * (pushed into their assignments), not here. Returns null otherwise.
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
        if (!assignmentsVisibleAt(sites, arg)) return null;
        // A single array-literal assignment expands to an `.add` chain.
        if (Node.isArrayLiteralExpression(sites[0].initializerNode)) {
            return collectArrayStrings(sites[0].initializerNode);
        }
        return null;
    }
    return null;
}

function collectArrayStrings(arr: ArrayLiteralExpression): string[] | null {
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
        // variable's value. The $p.s call site should still read `scale`
        // from the same binding the caller declared.
        const sites = assignments.get(arg.getText());
        if (!sites || sites.length !== 1) return null;
        if (!assignmentsVisibleAt(sites, arg)) return null;
        if (sites[0].kind !== 'string') return null;
        return arg.getText();
    }
    return null;
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
            push(map, nameNode.getText(), siteFromExpression(nameNode, init));
            return;
        }

        if (Node.isBinaryExpression(node)) {
            if (node.getOperatorToken().getText() !== '=') return;
            const left = node.getLeft();
            if (!Node.isIdentifier(left)) return;
            push(
                map,
                left.getText(),
                siteFromExpression(left, node.getRight()),
            );
        }
    });

    return map;
}

function siteFromExpression(nameNode: Node, expr: Expression): AssignmentSite {
    return {
        rhsStart: expr.getStart(),
        rhsEnd: expr.getEnd(),
        rhsText: expr.getText(),
        kind: isStringish(expr) ? 'string' : 'non-string',
        nameNode,
        initializerNode: expr,
    };
}

/**
 * The `varAssignments` map keys assignment sites by name only, so a name
 * shadowed by a different lexical binding (a block-scoped `let`, an inner
 * function parameter/declaration) would collect sites from unrelated
 * bindings. Resolve the call's referenced identifier to its declaration
 * symbol and require every site to resolve to that same symbol; if any
 * site binds elsewhere — or resolution fails — the name is ambiguous and
 * the caller must not rewrite it. This guarantees a rewrite only ever
 * touches the exact binding the call references.
 */
function assignmentsVisibleAt(
    sites: AssignmentSite[],
    callSite: Node,
): boolean {
    const callSym = callSite.getSymbol()?.compilerSymbol;
    if (!callSym) return false;
    return sites.every(
        (s) => s.nameNode.getSymbol()?.compilerSymbol === callSym,
    );
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
