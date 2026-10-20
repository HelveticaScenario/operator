/**
 * The two sinks of a panel gesture: the running module (at most one
 * single-module update per animation frame) and the call's literals in the
 * code (throttled while a control moves, flushed when the gesture ends, one
 * undo step per gesture).
 */
import { EQ_MODULE_TYPE } from '../../shared/dsl/eqPanelTypes';
import type { CodeStyle } from '../dsl/objectPropertyInsert';
import { isPerChannel, sectionLists } from './eqPanelModel';
import {
    type EqPanelContext,
    NOTICES,
    type PanelRuntime,
    parseSectionKey,
} from './eqPanelRuntime';
import { type EqSourceOp, computeEqSourceEdit } from './eqSourceEdit';
import type { EqListTarget, EqSourceEditResult } from './eqSourceLocate';

/** Interval between source writes while a control is moving. */
const CODE_WRITE_INTERVAL_MS = 150;

export function scheduleEngine(ctx: EqPanelContext, panel: PanelRuntime): void {
    if (panel.engineFrame !== null) {
        return;
    }
    panel.engineFrame = requestAnimationFrame(() => {
        panel.engineFrame = null;
        sendEngine(ctx, panel);
    });
}

export function sendEngineNow(ctx: EqPanelContext, panel: PanelRuntime): void {
    if (panel.engineFrame !== null) {
        cancelAnimationFrame(panel.engineFrame);
        panel.engineFrame = null;
    }
    sendEngine(ctx, panel);
}

function sendEngine(ctx: EqPanelContext, panel: PanelRuntime): void {
    ctx.setModuleParam(panel.def.moduleId, EQ_MODULE_TYPE, panel.params).catch(
        (err) => {
            console.error('EQ panel update failed:', err);
        },
    );
}

export function scheduleCode(ctx: EqPanelContext, panel: PanelRuntime): void {
    if (panel.codeTimer !== null) {
        return;
    }
    const elapsed = performance.now() - panel.lastCodeWrite;
    if (elapsed >= CODE_WRITE_INTERVAL_MS) {
        flushCode(ctx, panel, false);
        return;
    }
    panel.codeTimer = window.setTimeout(() => {
        panel.codeTimer = null;
        flushCode(ctx, panel, false);
    }, CODE_WRITE_INTERVAL_MS - elapsed);
}

/** Write pending literals now; `endGesture` closes the undo step. */
export function flushCode(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    endGesture: boolean,
): void {
    if (panel.codeTimer !== null) {
        window.clearTimeout(panel.codeTimer);
        panel.codeTimer = null;
    }
    if (panel.pendingCode.size > 0) {
        panel.lastCodeWrite = performance.now();
        const pending = [...panel.pendingCode];
        panel.pendingCode.clear();
        const lists = sectionLists(panel.params);
        const perChannel = isPerChannel(panel.params);
        for (const [key, fields] of pending) {
            const { list, index } = parseSectionKey(key);
            applySourceOp(
                ctx,
                panel,
                {
                    count: lists[list]?.length ?? 0,
                    list: perChannel ? list : null,
                    listCount: lists.length,
                },
                { fields, index, kind: 'set' },
                false,
            );
        }
    }
    if (endGesture) {
        ctx.setup?.model?.pushStackElement();
    }
}

/** Drop scheduled work; a pending engine send is superseded by the patch
 *  being committed. */
export function cancelTimers(panel: PanelRuntime): void {
    if (panel.codeTimer !== null) {
        window.clearTimeout(panel.codeTimer);
        panel.codeTimer = null;
    }
    if (panel.engineFrame !== null) {
        cancelAnimationFrame(panel.engineFrame);
        panel.engineFrame = null;
    }
}

/** Apply one section-list op at the panel's anchor; see `applyEdit`. */
export function applySourceOp(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    target: EqListTarget,
    op: EqSourceOp,
    closeUndo = true,
): boolean {
    return applyEdit(
        ctx,
        panel,
        (source, anchor, layout) =>
            computeEqSourceEdit(source, anchor, target, op, layout),
        closeUndo,
    );
}

/**
 * Compute an edit at the panel's anchor and apply it, recording a notice
 * when the code can't take it. Returns whether the code now holds the
 * change.
 */
export function applyEdit(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    compute: (
        source: string,
        anchor: number,
        layout: CodeStyle,
    ) => EqSourceEditResult,
    closeUndo = true,
): boolean {
    const model = ctx.setup?.model;
    const range = ctx.setup?.decorations?.getRange(panel.index);
    if (!ctx.codeStyle || !model || model.isDisposed()) {
        return false;
    }
    if (!range || range.isEmpty()) {
        panel.notice = NOTICES.missing;
        return false;
    }
    const anchor = model.getOffsetAt(range.getStartPosition());
    const result = compute(model.getValue(), anchor, ctx.codeStyle);
    if (!result.ok) {
        panel.notice = NOTICES[result.reason];
        return false;
    }
    if (result.skipped.length > 0) {
        panel.notice = NOTICES.skipped;
    }
    if (!result.edit) {
        return result.skipped.length === 0;
    }
    const start = model.getPositionAt(result.edit.start);
    const end = model.getPositionAt(result.edit.end);
    const ed = ctx.editor;
    const visible = ed !== null && ed.getModel() === model;
    const selections = visible ? (ed.getSelections() ?? []) : [];
    if (closeUndo) {
        model.pushStackElement();
    }
    model.pushEditOperations(
        selections,
        [
            {
                range: {
                    endColumn: end.column,
                    endLineNumber: end.lineNumber,
                    startColumn: start.column,
                    startLineNumber: start.lineNumber,
                },
                text: result.edit.text,
            },
        ],
        () => (visible ? selections : null),
    );
    if (closeUndo) {
        model.pushStackElement();
    }
    return true;
}
