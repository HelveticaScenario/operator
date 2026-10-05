/**
 * Locate DSL control calls and resolve the control group each is made on,
 * from the source alone.
 *
 * A control call is either free — `$slider`, `$btn`, `$toggleBtn`, `$cGroup`
 * (the root group) — or a `slider` / `btn` / `toggleBtn` / `cGroup` method
 * call on a statically known group: a `$cGroup(...)` call, a `.cGroup(...)`
 * call on a group, or a `const` bound to one of those. A method call on any
 * other receiver (a parameter, a `let`, a computed value) is not reported:
 * only evaluation could tell whether it is a group at all.
 *
 * Shared by the executor, which rejects controls created at a call site not
 * found here, and the control panel, which builds its group tree from it.
 */

import { ts } from 'ts-morph';

export type ControlKind = '$slider' | '$btn' | '$toggleBtn' | '$cGroup';

const FREE_KINDS = new Set<string>([
    '$slider',
    '$btn',
    '$toggleBtn',
    '$cGroup',
]);

/** Group method name → the control kind it creates. */
const METHOD_KINDS = new Map<string, ControlKind>([
    ['slider', '$slider'],
    ['btn', '$btn'],
    ['toggleBtn', '$toggleBtn'],
    ['cGroup', '$cGroup'],
]);

export interface ControlCall {
    kind: ControlKind;
    call: ts.CallExpression;
    /** The callee name node (`$slider`, `slider`, …) */
    name: ts.Node;
    /** Path of the group the call is made on: [] for free calls */
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

/** The control kind a call's callee names, and the callee's name node. */
function calleeOf(
    call: ts.CallExpression,
): { kind: ControlKind; name: ts.Node; receiver: ts.Expression | null } | null {
    const callee = call.expression;
    if (ts.isIdentifier(callee) && FREE_KINDS.has(callee.text)) {
        return {
            kind: callee.text as ControlKind,
            name: callee,
            receiver: null,
        };
    }
    const kind = ts.isPropertyAccessExpression(callee)
        ? METHOD_KINDS.get(callee.name.text)
        : undefined;
    if (kind && ts.isPropertyAccessExpression(callee)) {
        return { kind, name: callee.name, receiver: callee.expression };
    }
    return null;
}

/** Path of the group an expression evaluates to, or null if not static. */
function groupPathOf(expr: ts.Expression, scopes: Scope[]): string[] | null {
    if (ts.isParenthesizedExpression(expr)) {
        return groupPathOf(expr.expression, scopes);
    }
    if (ts.isIdentifier(expr)) {
        return lookup(scopes, expr.text);
    }
    if (!ts.isCallExpression(expr)) {
        return null;
    }
    const callee = calleeOf(expr);
    const label = expr.arguments[0];
    if (callee?.kind !== '$cGroup' || !label || !ts.isStringLiteral(label)) {
        return null;
    }
    const parent = callee.receiver ? groupPathOf(callee.receiver, scopes) : [];
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

    const statements =
        ts.isSourceFile(node) || ts.isBlock(node) || ts.isModuleBlock(node)
            ? node.statements
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
        if (ts.isCallExpression(node)) {
            const callee = calleeOf(node);
            const group = callee?.receiver
                ? groupPathOf(callee.receiver, scopes)
                : [];
            if (callee && group) {
                calls.push({
                    call: node,
                    group,
                    kind: callee.kind,
                    name: callee.name,
                });
            }
        }
        ts.forEachChild(node, visit);
        if (scoped) {
            scopes.pop();
        }
    };
    visit(sourceFile);
    // The walk visits a chain's outermost call first; order by where each
    // call's name appears instead.
    return calls.sort((a, b) => a.name.pos - b.name.pos);
}
