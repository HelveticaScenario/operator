/**
 * Monaco quick-fix provider that wraps module params in control calls
 * (`$slider`, `$btn`, `$toggleBtn`) chosen and seeded from the params'
 * schema metadata. Analysis lives in
 * src/renderer/dsl/controlQuickFix.ts; this file only adapts it to Monaco's
 * code-action surface and moves the caret past the inserted call once an
 * edit applies.
 */
import type { Monaco } from '../../hooks/useCustomMonaco';
import type { editor, languages } from 'monaco-editor';

import type { Schemas } from '../../../shared/dsl/schemaTypeResolver';
import type { CodeStyle } from '../../dsl/objectPropertyInsert';
import { computeControlQuickFixes } from '../../dsl/controlQuickFix';

const CURSOR_CMD = 'modular.placeCursorAfterControl';

/** Buffers larger than this skip analysis — a full parse per lightbulb
 *  request is only cheap for patch-sized sources. */
const MAX_ANALYZED_SOURCE_LENGTH = 100_000;

export function registerControlQuickFixProvider(
    monaco: Monaco,
    getSchemas: () => Schemas,
    getLayout: () => CodeStyle,
): { dispose: () => void } {
    // Runs after Monaco applies a code action's edit: put the caret just
    // past the inserted control call.
    const commandDisposable = monaco.editor.registerCommand(
        CURSOR_CMD,
        (_accessor: unknown, uriString: string, offset: number) => {
            const target = monaco.editor
                .getEditors()
                .find((e) => e.getModel()?.uri.toString() === uriString);
            const model = target?.getModel();
            if (!target || !model) {
                return;
            }
            const position = model.getPositionAt(offset);
            target.setPosition(position);
            target.revealPositionInCenterIfOutsideViewport(position);
            target.focus();
        },
    );

    const provider: languages.CodeActionProvider = {
        provideCodeActions(
            model: editor.ITextModel,
            range,
            _context,
            token,
        ): languages.CodeActionList {
            const empty = { actions: [], dispose() {} };
            // Patch buffers only — keybindings/config are json models, and
            // the DSL lib extra model is a .d.ts.
            const path = model.uri.path;
            if (!path.endsWith('.js') && !path.endsWith('.mjs')) {
                return empty;
            }
            const schemas = getSchemas();
            if (schemas.length === 0) {
                return empty;
            }
            if (model.getValueLength() > MAX_ANALYZED_SOURCE_LENGTH) {
                return empty;
            }

            const offset = model.getOffsetAt(range.getStartPosition());
            const fixes = computeControlQuickFixes(
                model.getValue(),
                offset,
                schemas,
                getLayout(),
            );
            if (token.isCancellationRequested) {
                return empty;
            }

            const actions: languages.CodeAction[] = fixes.map((fix) => ({
                command: {
                    arguments: [
                        model.uri.toString(),
                        // The edit replaces [span.start, span.end) with
                        // newText, so text before span.start keeps its
                        // offsets — the caret lands at start + caretOffset.
                        fix.span.start + fix.caretOffset,
                    ],
                    id: CURSOR_CMD,
                    title: 'Place cursor after control',
                },
                edit: {
                    edits: [
                        {
                            resource: model.uri,
                            textEdit: {
                                range: monaco.Range.fromPositions(
                                    model.getPositionAt(fix.span.start),
                                    model.getPositionAt(fix.span.end),
                                ),
                                text: fix.newText,
                            },
                            versionId: model.getVersionId(),
                        },
                    ],
                },
                isPreferred: fix.preferred ?? false,
                kind: 'quickfix',
                title: fix.title,
            }));
            return { actions, dispose() {} };
        },
    };

    const providerDisposable = monaco.languages.registerCodeActionProvider(
        'javascript',
        provider,
        { providedCodeActionKinds: ['quickfix'] },
    );

    return {
        dispose: () => {
            providerDisposable.dispose();
            commandDisposable.dispose();
        },
    };
}
