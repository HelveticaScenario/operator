/**
 * Dotted name of a call's callee, read from the syntax tree so the whitespace,
 * newlines, and comments a patch may put around the dots never reach it:
 * `$tr808\n  .bd` and `$tr808.bd` both name `$tr808.bd`.
 */
import { Node } from 'ts-morph';

/**
 * `foo` for an identifier, `a.b.c` for a property-access chain rooted at an
 * identifier, and null for anything else (a call or element access in the
 * chain, for instance).
 */
export function dottedCalleeName(expr: Node): string | null {
    if (Node.isIdentifier(expr)) {
        return expr.getText();
    }
    if (Node.isPropertyAccessExpression(expr)) {
        const base = dottedCalleeName(expr.getExpression());
        return base === null ? null : `${base}.${expr.getName()}`;
    }
    return null;
}
