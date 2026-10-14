/**
 * The panels of one patch run, built when the run is submitted and
 * committed when the engine applies it (immediately, or at a queued
 * bar/beat).
 */
import { editor } from 'monaco-editor';

import type { EqPanelDef } from '../../shared/dsl/eqPanelTypes';
import { FIRST_LINE_COLUMN_OFFSET } from '../../shared/dsl/spanTypes';
import type { DSLExecuteResult } from '../../shared/ipcTypes';
import { resolveScopeCallRange } from '../app/scopeCallRange';
import type { RawEqParams } from './eqPanelModel';

export interface EqPanelSetup {
    defs: EqPanelDef[];
    params: Map<string, RawEqParams>;
    model: editor.ITextModel | null;
    /** One tracked range per def, from the callee to the call's last line. */
    decorations: editor.IEditorDecorationsCollection | null;
    /**
     * Old module id → new module id for instances the engine carried over
     * under a new id; panels follow these so their state stays with the same
     * engine instance.
     */
    moduleIdRemap: Record<string, string>;
    /** The panel edit sequence when the run was submitted; edits after it are
     *  missing from this setup's params. */
    stagedSeq: number;
}

/** Build the setup for a patch run, anchored in the editor's model. */
export function createEqPanelSetup(
    result: DSLExecuteResult,
    editorInstance: editor.IStandaloneCodeEditor | null,
    stagedSeq: number,
): EqPanelSetup {
    const defs = result.eqPanels ?? [];
    const model = editorInstance?.getModel() ?? null;
    const params = new Map<string, RawEqParams>();
    for (const m of result.appliedPatch?.modules ?? []) {
        if (defs.some((d) => d.moduleId === m.id)) {
            const p = structuredClone(m.params) as RawEqParams;
            if (!Array.isArray(p.sections)) {
                p.sections = [];
            }
            params.set(m.id, p);
        }
    }
    const base = {
        defs,
        model,
        moduleIdRemap: result.moduleIdRemap ?? {},
        params,
        stagedSeq,
    };
    if (!editorInstance || !model || defs.length === 0) {
        return { ...base, decorations: null };
    }
    const descs: editor.IModelDeltaDecoration[] = defs.map((def) => {
        const loc = def.zoneLocation;
        const range = loc
            ? resolveScopeCallRange(
                  model,
                  {
                      column:
                          loc.line === 1
                              ? loc.column - FIRST_LINE_COLUMN_OFFSET
                              : loc.column,
                      line: loc.line,
                  },
                  result.callSiteSpans?.[`${loc.line}:${loc.column}`],
              )
            : null;
        return {
            options: {
                stickiness:
                    editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
            },
            range: range ?? {
                endColumn: 1,
                endLineNumber: 1,
                startColumn: 1,
                startLineNumber: 1,
            },
        };
    });
    return {
        ...base,
        decorations: editorInstance.createDecorationsCollection(descs),
    };
}

export function disposeEqPanelSetup(setup: EqPanelSetup | null): void {
    setup?.decorations?.clear();
}
