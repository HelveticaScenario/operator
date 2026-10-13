/**
 * Source edits for the panel's section controls: a section's property
 * values, an appended section (adding `sections` when absent), or a removed
 * one. Only literal values are rewritten; a section property holding any
 * other expression is left alone and reported as skipped.
 */
import { ts } from 'ts-morph';

import type { EqField } from './eqPanelModel';
import {
    type CodeStyle,
    insertObjectProperty,
} from '../dsl/objectPropertyInsert';
import {
    type EqListTarget,
    type EqSourceEditResult,
    type FieldLiteral,
    appendElement,
    applyTextEdit,
    diffEdit,
    findProperty,
    isRewritableLiteral,
    locate,
    removeElement,
} from './eqSourceLocate';

export type EqSourceOp =
    | {
          kind: 'set';
          index: number;
          fields: Partial<Record<EqField, FieldLiteral>>;
      }
    | { kind: 'add'; fields: Partial<Record<EqField, FieldLiteral>> }
    | { kind: 'remove'; index: number };

/** Source order of fields within a written section. */
const FIELD_ORDER: EqField[] = ['type', 'freq', 'gain', 'q'];

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
            source: applyTextEdit(source, {
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
        source: applyTextEdit(source, {
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
            edit: appendElement(loc.sf, loc.sections, text),
            ok: true,
            skipped: [],
        };
    }
    return {
        edit: loc.sections
            ? removeElement(loc.sf, loc.sections, op.index)
            : null,
        ok: true,
        skipped: [],
    };
}
