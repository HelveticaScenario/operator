/**
 * Locate DSL control calls and resolve the control group each belongs to,
 * from the source alone.
 *
 * A control call is a `$slider`, `$btn`, `$toggleBtn`, or `$cGroup` call. Its
 * optional group argument (`$cGroup`'s is the `group` property of its params
 * object) must be statically known: a `$cGroup(...)` call or a `const` bound
 * to one. A call whose group is any other expression (a parameter, a `let`,
 * a computed value) is not reported: only evaluation could tell which group
 * it is, if any.
 *
 * Shared by the executor, which rejects controls created at a call site not
 * found here, and the control panel, which builds its group tree from it.
 */

import { ts } from 'ts-morph';

export type ControlKind = '$slider' | '$btn' | '$toggleBtn' | '$cGroup';

/** Positional index of each control's optional group argument. `$cGroup`
 *  takes its group in the params object at index 1 instead. */
export const GROUP_ARG_INDEX = {
    $btn: 1,
    $slider: 4,
    $toggleBtn: 2,
} as const;

const KINDS = new Set<string>(['$slider', '$btn', '$toggleBtn', '$cGroup']);

export interface ControlCall {
    kind: ControlKind;
    call: ts.CallExpression;
    /** The callee name node (`$slider`, …) */
    name: ts.Node;
    /** Path of the group the control belongs to: [] when it has none */
    group: string[];
}

/** Scope chain: name → group path, or null for a binding that is not a
 *  statically known group (it shadows any outer group of the same name). */
type Scope = Map<string, string[] | null>;

function lookup(scopes: Scope[], name: string): string[] | null {
    for (let i = scopes.length - 1; i >= 0; i--) {
        const bound = scopes[i].get(name);
        if (bound !== undefined) {
            return bound;
        }
    }
    return null;
}

/** The control kind a call's callee names, or null for any other call. */
function kindOf(call: ts.CallExpression): ControlKind | null {
    const callee = call.expression;
    return ts.isIdentifier(callee) && KINDS.has(callee.text)
        ? (callee.text as ControlKind)
        : null;
}

/** The `group` property of a `$cGroup` params object literal, if any. */
function groupProperty(
    params: ts.ObjectLiteralExpression,
): ts.ObjectLiteralElementLike | undefined {
    return params.properties.find(
        (p) =>
            (ts.isPropertyAssignment(p) ||
                ts.isShorthandPropertyAssignment(p)) &&
            propertyName(p) === 'group',
    );
}

/** A property's static key, or null for a computed or spread member. */
export function propertyName(p: ts.ObjectLiteralElementLike): string | null {
    if (ts.isSpreadAssignment(p) || !p.name) {
        return null;
    }
    return ts.isIdentifier(p.name) ||
        ts.isStringLiteral(p.name) ||
        ts.isNumericLiteral(p.name)
        ? p.name.text
        : null;
}

/**
 * Path of the group a control call belongs to: [] without a group argument,
 * or null when that argument is not a statically known group.
 */
function groupOfCall(
    call: ts.CallExpression,
    kind: ControlKind,
    scopes: Scope[],
): string[] | null {
    if (kind !== '$cGroup') {
        const arg = call.arguments[GROUP_ARG_INDEX[kind]];
        return arg ? groupPathOf(arg, scopes) : [];
    }
    const params = call.arguments[1];
    if (!params) {
        return [];
    }
    if (!ts.isObjectLiteralExpression(params)) {
        return null;
    }
    // A spread, computed key, method, or accessor could supply the group out
    // of sight of the scan.
    const readable = params.properties.every(
        (p) =>
            (ts.isPropertyAssignment(p) || ts.isShorthandPropertyAssignment(p)) &&
            propertyName(p) !== null,
    );
    if (!readable) {
        return null;
    }
    const prop = groupProperty(params);
    if (!prop) {
        return [];
    }
    return ts.isShorthandPropertyAssignment(prop)
        ? lookup(scopes, prop.name.text)
        : groupPathOf((prop as ts.PropertyAssignment).initializer, scopes);
}

/** Path of the group an expression evaluates to, or null if not static. */
function groupPathOf(expr: ts.Expression, scopes: Scope[]): string[] | null {
    if (ts.isParenthesizedExpression(expr)) {
        return groupPathOf(expr.expression, scopes);
    }
    if (ts.isIdentifier(expr)) {
        return lookup(scopes, expr.text);
    }
    if (!ts.isCallExpression(expr) || kindOf(expr) !== '$cGroup') {
        return null;
    }
    const label = expr.arguments[0];
    if (!label || !ts.isStringLiteral(label)) {
        return null;
    }
    const parent = groupOfCall(expr, '$cGroup', scopes);
    return parent ? [...parent, label.text] : null;
}

/** Names a binding pattern declares. */
function boundNames(name: ts.BindingName): string[] {
    if (ts.isIdentifier(name)) {
        return [name.text];
    }
    return name.elements.flatMap((e) =>
        ts.isOmittedExpression(e) ? [] : boundNames(e.name),
    );
}

/**
 * Declare a new scope's bindings up front, so a function body can use a
 * group const declared later in the enclosing scope. Only a `const` bound
 * directly to a group expression resolves; every other binding shadows.
 */
function declareScope(node: ts.Node, scopes: Scope[]): Scope {
    const scope: Scope = new Map();
    scopes.push(scope);

    if (ts.isFunctionLike(node)) {
        for (const param of node.parameters) {
            for (const name of boundNames(param.name)) {
                scope.set(name, null);
            }
        }
    }

    // Loop-head and catch bindings are never statically known groups.
    if (
        (ts.isForStatement(node) ||
            ts.isForInStatement(node) ||
            ts.isForOfStatement(node)) &&
        node.initializer &&
        ts.isVariableDeclarationList(node.initializer)
    ) {
        for (const decl of node.initializer.declarations) {
            for (const name of boundNames(decl.name)) {
                scope.set(name, null);
            }
        }
    }
    if (ts.isCatchClause(node) && node.variableDeclaration) {
        for (const name of boundNames(node.variableDeclaration.name)) {
            scope.set(name, null);
        }
    }

    // A switch's case clauses share one block scope.
    const statements =
        ts.isSourceFile(node) || ts.isBlock(node) || ts.isModuleBlock(node)
            ? node.statements
            : ts.isCaseBlock(node)
              ? node.clauses.flatMap((c) => [...c.statements])
              : [];
    for (const statement of statements) {
        if (ts.isVariableStatement(statement)) {
            const isConst =
                (statement.declarationList.flags & ts.NodeFlags.Const) !== 0;
            for (const decl of statement.declarationList.declarations) {
                if (isConst && ts.isIdentifier(decl.name) && decl.initializer) {
                    scope.set(
                        decl.name.text,
                        groupPathOf(decl.initializer, scopes),
                    );
                } else {
                    for (const name of boundNames(decl.name)) {
                        scope.set(name, null);
                    }
                }
            }
        } else if (ts.isFunctionDeclaration(statement) && statement.name) {
            scope.set(statement.name.text, null);
        } else if (ts.isClassDeclaration(statement) && statement.name) {
            scope.set(statement.name.text, null);
        }
    }
    return scope;
}

function opensScope(node: ts.Node): boolean {
    return (
        ts.isSourceFile(node) ||
        ts.isBlock(node) ||
        ts.isModuleBlock(node) ||
        ts.isCaseBlock(node) ||
        ts.isCatchClause(node) ||
        ts.isForStatement(node) ||
        ts.isForInStatement(node) ||
        ts.isForOfStatement(node) ||
        ts.isFunctionLike(node)
    );
}

/** Every control call in the file, in order of their names in the text. */
export function findControlCalls(sourceFile: ts.SourceFile): ControlCall[] {
    const calls: ControlCall[] = [];
    const scopes: Scope[] = [];

    const visit = (node: ts.Node) => {
        const scoped = opensScope(node);
        if (scoped) {
            declareScope(node, scopes);
        }
        const kind = ts.isCallExpression(node) ? kindOf(node) : null;
        if (kind) {
            const call = node as ts.CallExpression;
            const group = groupOfCall(call, kind, scopes);
            if (group) {
                calls.push({ call, group, kind, name: call.expression });
            }
        }
        ts.forEachChild(node, visit);
        if (scoped) {
            scopes.pop();
        }
    };
    visit(sourceFile);
    // The walk visits an enclosing call before a control call nested in its
    // arguments; order by where each call's name appears instead.
    return calls.sort((a, b) => a.name.pos - b.name.pos);
}
