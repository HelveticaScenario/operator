/**
 * Source edits behind the EQ panel's controls. Each edit parses the current
 * document, finds the `$unstable.eq(...)` call by the offset of its `eq`
 * method name (the panel's tracked anchor), and rewrites a section list
 * literal in its config's `sections` — the array itself, or one channel's
 * nested array in the per-channel form: a section's property values, an
 * appended section (adding `sections` when absent), or a removed one. Only literal values are rewritten; a section property holding any
 * other expression is left alone and reported as skipped.
 */
import { ts } from 'ts-morph';

import type { EqField } from './eqPanelModel';
import {
    type CodeStyle,
    eolOf,
    fitsOnLine,
    indentAt,
    indentUnit,
    insertObjectProperty,
    lineEnd,
} from '../dsl/objectPropertyInsert';

export interface TextEdit {
    /** Half-open [start, end) replacement span. */
    start: number;
    end: number;
    text: string;
}

/** A field's new source literal, given the quote to use for strings. */
export type FieldLiteral = (quote: string) => string;

export type EqSourceOp =
    | {
          kind: 'set';
          index: number;
          fields: Partial<Record<EqField, FieldLiteral>>;
      }
    | { kind: 'add'; fields: Partial<Record<EqField, FieldLiteral>> }
    | { kind: 'remove'; index: number };

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

/** Source order of fields within a written section. */
const FIELD_ORDER: EqField[] = ['type', 'freq', 'gain', 'q'];

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
            ts.isPropertyAccessExpression(node.expression) &&
            node.expression.name.text === 'eq' &&
            node.expression.name.getStart(sf) === anchor
        ) {
            found = node;
            return;
        }
        ts.forEachChild(node, visit);
    };
    visit(sf);
    return found;
}

type Located =
    | { ok: true; sf: ts.SourceFile; sections: ts.ArrayLiteralExpression }
    /** No `sections` yet: an empty shared list, added on the first append. */
    | {
          ok: true;
          sf: ts.SourceFile;
          sections: null;
          config: ts.ObjectLiteralExpression;
      }
    | { ok: false; reason: 'stale' | 'unsupported' };

type LocatedSections =
    | {
          ok: true;
          sf: ts.SourceFile;
          config: ts.ObjectLiteralExpression;
          /** The `sections` array literal, or null when absent. */
          outer: ts.ArrayLiteralExpression | null;
      }
    | { ok: false; reason: 'stale' | 'unsupported' };

/** Find the call's config (the object holding `ui`) and its sections. */
function locateSections(source: string, anchor: number): LocatedSections {
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

function isPerChannelLiteral(outer: ts.ArrayLiteralExpression): boolean {
    const first = outer.elements[0];
    return first !== undefined && ts.isArrayLiteralExpression(first);
}

/** Find the addressed list literal, checking it has the expected shape. */
function locate(source: string, anchor: number, target: EqListTarget): Located {
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

function findProperty(
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

function isRewritableLiteral(node: ts.Expression): boolean {
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

function applyEdit(source: string, edit: TextEdit): string {
    return source.slice(0, edit.start) + edit.text + source.slice(edit.end);
}

/** The single edit turning `before` into `after`. */
function diffEdit(before: string, after: string): TextEdit | null {
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

/** Set one field of section `index`, or report it unwritable. */
function setField(
    source: string,
    anchor: number,
    target: EqListTarget,
    index: number,
    field: EqField,
    literal: FieldLiteral,
    layout: CodeStyle,
): { source: string } | { skipped: true } | { fail: 'stale' | 'unsupported' } {
    const loc = locate(source, anchor, target);
    if (!loc.ok) {
        return { fail: loc.reason };
    }
    if (!loc.sections) {
        return { fail: 'stale' };
    }
    const element = loc.sections.elements[index];
    if (!element || !ts.isObjectLiteralExpression(element)) {
        return { skipped: true };
    }
    const prop = findProperty(element, field);
    if (prop) {
        if (
            !ts.isPropertyAssignment(prop) ||
            !isRewritableLiteral(prop.initializer)
        ) {
            return { skipped: true };
        }
        const init = prop.initializer;
        const start = init.getStart(loc.sf);
        const quote =
            ts.isStringLiteral(init) || ts.isNoSubstitutionTemplateLiteral(init)
                ? source[start]
                : layout.quote;
        return {
            source: applyEdit(source, {
                end: init.getEnd(),
                start,
                text: literal(quote),
            }),
        };
    }
    const insert = insertObjectProperty(
        loc.sf,
        element,
        `${field}: ${literal(layout.quote)}`,
        layout,
    );
    return {
        source: applyEdit(source, {
            end: insert.span.end,
            start: insert.span.start,
            text: insert.newText,
        }),
    };
}

/** Section object literal text for an appended section. */
function sectionText(
    fields: Partial<Record<EqField, FieldLiteral>>,
    layout: CodeStyle,
): string {
    const pad = layout.bracketSpacing ? ' ' : '';
    const props = FIELD_ORDER.filter((f) => fields[f] !== undefined).map(
        (f) => `${f}: ${fields[f]!(layout.quote)}`,
    );
    return `{${pad}${props.join(', ')}${pad}}`;
}

function appendSection(
    sf: ts.SourceFile,
    sections: ts.ArrayLiteralExpression,
    text: string,
): TextEdit {
    const source = sf.text;
    const elements = sections.elements;
    const close = sections.getEnd() - 1;
    if (elements.length === 0) {
        return { end: close, start: sections.getStart(sf) + 1, text };
    }
    const last = elements[elements.length - 1];
    const comma = /^\s*,/.exec(source.slice(last.getEnd(), close));
    const expanded = source
        .slice(sections.getStart(sf) + 1, elements[0].getStart(sf))
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
 * element's end, so a trailing comma (now the previous element's) remains.
 * Removing the only element leaves `[]`.
 */
function removeSection(
    sf: ts.SourceFile,
    sections: ts.ArrayLiteralExpression,
    index: number,
): TextEdit | null {
    const elements = sections.elements;
    if (index < 0 || index >= elements.length) {
        return null;
    }
    if (elements.length === 1) {
        return {
            end: sections.getEnd() - 1,
            start: sections.getStart(sf) + 1,
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

/**
 * Compute the edit for `op` on the list `target` of the EQ call anchored at
 * `anchor`. A list whose shape differs from `target` means the code diverged
 * by hand and indices are unsafe.
 */
export function computeEqSourceEdit(
    source: string,
    anchor: number,
    target: EqListTarget,
    op: EqSourceOp,
    layout: CodeStyle,
): EqSourceEditResult {
    if (op.kind === 'set') {
        let current = source;
        const skipped: EqField[] = [];
        for (const field of FIELD_ORDER) {
            const literal = op.fields[field];
            if (!literal) {
                continue;
            }
            const res = setField(
                current,
                anchor,
                target,
                op.index,
                field,
                literal,
                layout,
            );
            if ('fail' in res) {
                return { ok: false, reason: res.fail };
            }
            if ('skipped' in res) {
                skipped.push(field);
            } else {
                current = res.source;
            }
        }
        return { edit: diffEdit(source, current), ok: true, skipped };
    }

    const loc = locate(source, anchor, target);
    if (!loc.ok) {
        return loc;
    }
    if (op.kind === 'add') {
        const text = sectionText(op.fields, layout);
        if (!loc.sections) {
            const insert = insertObjectProperty(
                loc.sf,
                loc.config,
                `sections: [${text}]`,
                layout,
            );
            return {
                edit: {
                    end: insert.span.end,
                    start: insert.span.start,
                    text: insert.newText,
                },
                ok: true,
                skipped: [],
            };
        }
        return {
            edit: appendSection(loc.sf, loc.sections, text),
            ok: true,
            skipped: [],
        };
    }
    return {
        edit: loc.sections
            ? removeSection(loc.sf, loc.sections, op.index)
            : null,
        ok: true,
        skipped: [],
    };
}

/** A change to the channel lists themselves. */
export type EqChannelOp =
    /** Shared list → two per-channel copies of it. */
    | { kind: 'split' }
    /** Append a copy of channel list `from`. */
    | { kind: 'addChannel'; from: number }
    /** Remove channel list `index`; one remaining list becomes the shared form. */
    | { kind: 'removeChannel'; index: number };

/** Shift every line after the first by `delta` indent units (±1). */
function reindent(text: string, unit: string, delta: 1 | -1): string {
    if (delta === 1) {
        return text.replace(/\n/g, `\n${unit}`);
    }
    const escaped = unit.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    return text.replace(new RegExp(`\n${escaped}`, 'g'), '\n');
}

/**
 * Compute the edit for a channel-list op. `listCount` is the number of
 * channel lists the panel holds, 0 for the shared form.
 */
export function computeEqChannelEdit(
    source: string,
    anchor: number,
    listCount: number,
    op: EqChannelOp,
    layout: CodeStyle,
): EqSourceEditResult {
    const found = locateSections(source, anchor);
    if (!found.ok) {
        return found;
    }
    const { config, outer, sf } = found;
    const perChannel = outer !== null && isPerChannelLiteral(outer);
    const unit = indentUnit(layout);
    const nl = eolOf(source);

    if (op.kind === 'split') {
        if (perChannel || listCount !== 0) {
            return { ok: false, reason: 'stale' };
        }
        if (!outer) {
            const insert = insertObjectProperty(
                sf,
                config,
                'sections: [[], []]',
                layout,
            );
            return {
                edit: {
                    end: insert.span.end,
                    start: insert.span.start,
                    text: insert.newText,
                },
                ok: true,
                skipped: [],
            };
        }
        const start = outer.getStart(sf);
        const list = source.slice(start, outer.getEnd());
        const span = { end: outer.getEnd(), start };
        const inline = `[${list}, ${list}]`;
        if (
            !list.includes('\n') &&
            fitsOnLine(source, { newText: inline, propOffset: 0, span }, layout)
        ) {
            return { edit: { ...span, text: inline }, ok: true, skipped: [] };
        }
        const base = indentAt(source, start);
        const copy = `${base}${unit}${reindent(list, unit, 1)},`;
        return {
            edit: { ...span, text: `[${nl}${copy}${nl}${copy}${nl}${base}]` },
            ok: true,
            skipped: [],
        };
    }

    if (!outer || !perChannel || outer.elements.length !== listCount) {
        return { ok: false, reason: 'stale' };
    }
    if (op.kind === 'addChannel') {
        const from = outer.elements[op.from];
        if (!from || !ts.isArrayLiteralExpression(from)) {
            return { ok: false, reason: 'unsupported' };
        }
        return {
            edit: appendSection(sf, outer, from.getText(sf)),
            ok: true,
            skipped: [],
        };
    }
    if (listCount === 2) {
        const keep = outer.elements[op.index === 0 ? 1 : 0];
        return {
            edit: {
                end: outer.getEnd(),
                start: outer.getStart(sf),
                text: reindent(keep.getText(sf), unit, -1),
            },
            ok: true,
            skipped: [],
        };
    }
    return {
        edit: removeSection(sf, outer, op.index),
        ok: true,
        skipped: [],
    };
}
