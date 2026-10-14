/**
 * Source edits for the panel's channel-list controls: splitting the shared
 * list into per-channel copies, adding a copy of a channel's list, and
 * removing one (two remaining lists collapse back to the shared form).
 */
import { ts } from 'ts-morph';

import {
    type CodeStyle,
    eolOf,
    fitsOnLine,
    indentAt,
    indentUnit,
    insertObjectProperty,
} from '../dsl/objectPropertyInsert';
import {
    type EqSourceEditResult,
    appendElement,
    isPerChannelLiteral,
    locateSections,
    removeElement,
} from './eqSourceLocate';

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
            edit: appendElement(sf, outer, from.getText(sf)),
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
        edit: removeElement(sf, outer, op.index),
        ok: true,
        skipped: [],
    };
}
