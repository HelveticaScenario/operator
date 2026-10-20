/**
 * Finding an EQ call in the source and the list literals the panel edits.
 * Each edit parses the current document and finds the call by the offset of
 * its callee — `$eq` itself, or the `eq` of a `.$.eq(...)` chain (the
 * panel's tracked anchor) — then the config object holding `ui` and its
 * `sections`: the array itself, or one channel's nested array.
 */
import { ts } from 'ts-morph';

import type { EqField } from './eqPanelModel';
import { eolOf, indentAt, lineEnd } from '../dsl/objectPropertyInsert';

export interface TextEdit {
    /** Half-open [start, end) replacement span. */
    start: number;
    end: number;
    text: string;
}

/** A field's new source literal, given the quote to use for strings. */
export type FieldLiteral = (quote: string) => string;

/** The section list an op addresses, with the shape the panel expects. */
export interface EqListTarget {
    /** Channel list index, or null for the shared (one-list) form. */
    list: number | null;
    /** Channel lists in the per-channel form (ignored when shared). */
    listCount: number;
    /** Sections in the addressed list before the op. */
    count: number;
}

export type EqSourceEditResult =
    | { ok: true; edit: TextEdit | null; skipped: EqField[] }
    /**
     * `stale`: the call or its sections no longer match the panel (moved
     * anchor, a form or count changed by hand). `unsupported`: the addressed
     * list is not an array literal.
     */
    | { ok: false; reason: 'stale' | 'unsupported' };

function parse(source: string): ts.SourceFile {
    return ts.createSourceFile(
        'patch.js',
        source,
        ts.ScriptTarget.Latest,
        true,
        ts.ScriptKind.JS,
    );
}

function findEqCall(
    sf: ts.SourceFile,
    anchor: number,
): ts.CallExpression | null {
    let found: ts.CallExpression | null = null;
    const visit = (node: ts.Node): void => {
        if (found || anchor < node.getFullStart() || anchor > node.getEnd()) {
            return;
        }
        if (
            ts.isCallExpression(node) &&
            ((ts.isIdentifier(node.expression) &&
                node.expression.text === '$eq' &&
                node.expression.getStart(sf) === anchor) ||
                (ts.isPropertyAccessExpression(node.expression) &&
                    node.expression.name.text === 'eq' &&
                    node.expression.name.getStart(sf) === anchor))
        ) {
            found = node;
            return;
        }
        ts.forEachChild(node, visit);
    };
    visit(sf);
    return found;
}

export type Located =
    | { ok: true; sf: ts.SourceFile; sections: ts.ArrayLiteralExpression }
    /** No `sections` yet: an empty shared list, added on the first append. */
    | {
          ok: true;
          sf: ts.SourceFile;
          sections: null;
          config: ts.ObjectLiteralExpression;
      }
    | { ok: false; reason: 'stale' | 'unsupported' };

export type LocatedSections =
    | {
          ok: true;
          sf: ts.SourceFile;
          config: ts.ObjectLiteralExpression;
          /** The `sections` array literal, or null when absent. */
          outer: ts.ArrayLiteralExpression | null;
      }
    | { ok: false; reason: 'stale' | 'unsupported' };

/** Find the call's config (the object holding `ui`) and its sections. */
export function locateSections(
    source: string,
    anchor: number,
): LocatedSections {
    const sf = parse(source);
    const call = findEqCall(sf, anchor);
    if (!call) {
        return { ok: false, reason: 'stale' };
    }
    const config = call.arguments.find(
        (a): a is ts.ObjectLiteralExpression =>
            ts.isObjectLiteralExpression(a) && findProperty(a, 'ui') !== null,
    );
    if (!config) {
        return { ok: false, reason: 'unsupported' };
    }
    const prop = findProperty(config, 'sections');
    if (!prop) {
        return { config, ok: true, outer: null, sf };
    }
    const outer = ts.isPropertyAssignment(prop) ? prop.initializer : null;
    if (!outer || !ts.isArrayLiteralExpression(outer)) {
        return { ok: false, reason: 'unsupported' };
    }
    return { config, ok: true, outer, sf };
}

export function isPerChannelLiteral(outer: ts.ArrayLiteralExpression): boolean {
    const first = outer.elements[0];
    return first !== undefined && ts.isArrayLiteralExpression(first);
}

/** Find the addressed list literal, checking it has the expected shape. */
export function locate(
    source: string,
    anchor: number,
    target: EqListTarget,
): Located {
    const found = locateSections(source, anchor);
    if (!found.ok) {
        return found;
    }
    const { config, outer, sf } = found;
    if (!outer) {
        return target.list === null && target.count === 0
            ? { config, ok: true, sections: null, sf }
            : { ok: false, reason: 'stale' };
    }
    let list = outer;
    if (target.list !== null) {
        if (outer.elements.length !== target.listCount) {
            return { ok: false, reason: 'stale' };
        }
        const inner = outer.elements[target.list];
        if (!ts.isArrayLiteralExpression(inner)) {
            return { ok: false, reason: 'unsupported' };
        }
        list = inner;
    } else if (isPerChannelLiteral(outer)) {
        return { ok: false, reason: 'stale' };
    }
    if (list.elements.length !== target.count) {
        return { ok: false, reason: 'stale' };
    }
    return { ok: true, sections: list, sf };
}

function propertyNameText(name: ts.PropertyName): string | null {
    if (ts.isIdentifier(name) || ts.isStringLiteral(name)) {
        return name.text;
    }
    return null;
}

export function findProperty(
    obj: ts.ObjectLiteralExpression,
    name: string,
): ts.ObjectLiteralElementLike | null {
    for (const p of obj.properties) {
        if (
            (ts.isPropertyAssignment(p) ||
                ts.isShorthandPropertyAssignment(p)) &&
            propertyNameText(p.name) === name
        ) {
            return p;
        }
    }
    return null;
}

export function isRewritableLiteral(node: ts.Expression): boolean {
    if (
        ts.isNumericLiteral(node) ||
        ts.isStringLiteral(node) ||
        ts.isNoSubstitutionTemplateLiteral(node)
    ) {
        return true;
    }
    return (
        ts.isPrefixUnaryExpression(node) &&
        (node.operator === ts.SyntaxKind.MinusToken ||
            node.operator === ts.SyntaxKind.PlusToken) &&
        ts.isNumericLiteral(node.operand)
    );
}

export function applyTextEdit(source: string, edit: TextEdit): string {
    return source.slice(0, edit.start) + edit.text + source.slice(edit.end);
}

/** The single edit turning `before` into `after`. */
export function diffEdit(before: string, after: string): TextEdit | null {
    if (before === after) {
        return null;
    }
    let start = 0;
    const maxStart = Math.min(before.length, after.length);
    while (start < maxStart && before[start] === after[start]) {
        start++;
    }
    let endBefore = before.length;
    let endAfter = after.length;
    while (
        endBefore > start &&
        endAfter > start &&
        before[endBefore - 1] === after[endAfter - 1]
    ) {
        endBefore--;
        endAfter--;
    }
    return { end: endBefore, start, text: after.slice(start, endAfter) };
}

/** Append `text` as the array's last element, matching its layout. */
export function appendElement(
    sf: ts.SourceFile,
    array: ts.ArrayLiteralExpression,
    text: string,
): TextEdit {
    const source = sf.text;
    const elements = array.elements;
    const close = array.getEnd() - 1;
    if (elements.length === 0) {
        return { end: close, start: array.getStart(sf) + 1, text };
    }
    const last = elements[elements.length - 1];
    const comma = /^\s*,/.exec(source.slice(last.getEnd(), close));
    const expanded = source
        .slice(array.getStart(sf) + 1, elements[0].getStart(sf))
        .includes('\n');
    if (expanded) {
        const nl = eolOf(source);
        const indent = indentAt(source, last.getStart(sf));
        if (comma) {
            const afterComma = last.getEnd() + comma[0].length;
            const eol = lineEnd(source, afterComma);
            const at = eol < close ? eol : afterComma;
            return { end: at, start: at, text: `${nl}${indent}${text},` };
        }
        return {
            end: last.getEnd(),
            start: last.getEnd(),
            text: `,${nl}${indent}${text}`,
        };
    }
    if (comma) {
        const at = last.getEnd() + comma[0].length;
        return { end: at, start: at, text: ` ${text}` };
    }
    return { end: last.getEnd(), start: last.getEnd(), text: `, ${text}` };
}

/**
 * Removes element `index` with its separator. A middle or first element is
 * cut up to the next element's start, so the next element takes over its
 * position and indentation; the last element is cut from the previous
 * element's end, so a trailing comma stays after the new last element.
 * Removing the only element leaves `[]`.
 */
export function removeElement(
    sf: ts.SourceFile,
    array: ts.ArrayLiteralExpression,
    index: number,
): TextEdit | null {
    const elements = array.elements;
    if (index < 0 || index >= elements.length) {
        return null;
    }
    if (elements.length === 1) {
        return {
            end: array.getEnd() - 1,
            start: array.getStart(sf) + 1,
            text: '',
        };
    }
    if (index < elements.length - 1) {
        return {
            end: elements[index + 1].getStart(sf),
            start: elements[index].getStart(sf),
            text: '',
        };
    }
    return {
        end: elements[index].getEnd(),
        start: elements[index - 1].getEnd(),
        text: '',
    };
}
