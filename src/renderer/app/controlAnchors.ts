import { FIRST_LINE_COLUMN_OFFSET } from '../../shared/dsl/spanTypes';

interface AnchorRange {
    startLineNumber: number;
    startColumn: number;
    endLineNumber: number;
    endColumn: number;
}

/** Minimal Monaco text-model surface needed to anchor control calls. */
export interface AnchorModel {
    getLineCount(): number;
    getLineContent(lineNumber: number): string;
    getOffsetAt(position: { lineNumber: number; column: number }): number;
    getDecorationRange(id: string): AnchorRange | null;
    deltaDecorations(
        oldDecorations: string[],
        newDecorations: {
            range: AnchorRange;
            options: { stickiness?: number };
        }[],
    ): string[];
    isDisposed(): boolean;
}

/** A running control whose call site should be anchored. */
export interface AnchoredControl {
    moduleId: string;
    /** The factory's free-function name, e.g. `$slider`. A group method call
     *  spells it without the `$` (`g.slider(...)`). */
    fnName: string;
    sourceLocation?: { line: number; column: number };
}

/** Monaco's TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges. */
const NEVER_GROWS_WHEN_TYPING_AT_EDGES = 1;

/**
 * Tracked anchors on the callee names of a running patch's control calls, in
 * the model the patch was evaluated from. Monaco moves the anchors as that
 * buffer is edited, so a running control stays bound to its call however the
 * code around it — including the call's own label — changes.
 */
export class ControlAnchors {
    constructor(
        private readonly model: AnchorModel,
        /** moduleId → decoration id and the callee name's length */
        private readonly anchors: Map<string, { id: string; length: number }>,
    ) {}

    /**
     * Offset of the running control's call in its model, or null when the
     * control has no anchor or its callee name has been edited or deleted.
     */
    offsetOf(moduleId: string): number | null {
        const anchor = this.anchors.get(moduleId);
        if (!anchor || this.model.isDisposed()) {
            return null;
        }
        const range = this.model.getDecorationRange(anchor.id);
        if (!range) {
            return null;
        }
        const start = this.model.getOffsetAt({
            column: range.startColumn,
            lineNumber: range.startLineNumber,
        });
        const end = this.model.getOffsetAt({
            column: range.endColumn,
            lineNumber: range.endLineNumber,
        });
        return end - start === anchor.length ? start : null;
    }

    dispose(): void {
        if (!this.model.isDisposed()) {
            this.model.deltaDecorations(
                [...this.anchors.values()].map((a) => a.id),
                [],
            );
        }
    }
}

/**
 * Anchor each control's call site in `model`. Locations come from the
 * submit-time source; a call site the live document no longer has at that
 * position (edited during the async round-trip) gets no anchor.
 */
export function createControlAnchors(
    model: AnchorModel,
    controls: AnchoredControl[],
): ControlAnchors {
    const placed: { moduleId: string; range: AnchorRange; length: number }[] =
        [];
    for (const { moduleId, fnName, sourceLocation: loc } of controls) {
        if (!loc || loc.line > model.getLineCount()) {
            continue;
        }
        const column =
            loc.line === 1 ? loc.column - FIRST_LINE_COLUMN_OFFSET : loc.column;
        const lineContent = model.getLineContent(loc.line);
        const name = [fnName, fnName.slice(1)].find(
            (n) => lineContent.slice(column - 1, column - 1 + n.length) === n,
        );
        if (!name) {
            continue;
        }
        placed.push({
            length: name.length,
            moduleId,
            range: {
                endColumn: column + name.length,
                endLineNumber: loc.line,
                startColumn: column,
                startLineNumber: loc.line,
            },
        });
    }

    const ids = model.deltaDecorations(
        [],
        placed.map((p) => ({
            options: { stickiness: NEVER_GROWS_WHEN_TYPING_AT_EDGES },
            range: p.range,
        })),
    );
    return new ControlAnchors(
        model,
        new Map(
            placed.map((p, i) => [
                p.moduleId,
                { id: ids[i], length: p.length },
            ]),
        ),
    );
}
