/**
 * `when`-clause diagnostics for the user keybindings.json editor buffer.
 *
 * Operator's when-clause grammar supports a subset of VS Code's operators
 * (`&& || ! == != ( )`). A clause using an unsupported operator (`=~`, `<`,
 * `in`, …) throws when parsed and is then treated as "not matching" at
 * dispatch, so the binding silently never fires. This surfaces that failure in
 * the editor as a warning marker instead, scoped to the keybindings buffer.
 */
import { parseTree, type Node } from 'jsonc-parser';
import type { Monaco } from '../../hooks/useCustomMonaco';
import type { editor } from 'monaco-editor';
import { parseWhen } from '../../keybindings/whenParser';

const MARKER_OWNER = 'keybindings-when';
const SUPPORTED_OPERATORS = '&& || ! == != ( )';

export interface WhenDiagnostic {
    message: string;
    /** Offset of the `when` value (the quoted string) in the document. */
    offset: number;
    length: number;
}

/** Validate a single `when` value; returns an error message or null if valid. */
function validateWhenValue(when: string): string | null {
    try {
        parseWhen(when);
        return null;
    } catch (err) {
        const detail =
            err instanceof Error
                ? err.message.replace(/^\[whenParser\]\s*/, '')
                : String(err);
        return `Invalid \`when\` clause: ${detail}. Supported operators: ${SUPPORTED_OPERATORS}.`;
    }
}

/**
 * Parse the keybindings JSON and return a diagnostic for every `when` value
 * that fails to parse. Tolerant of malformed JSON (jsonc-parser returns a
 * best-effort tree), so it never throws on partially-typed buffers.
 */
export function validateWhenClauses(text: string): WhenDiagnostic[] {
    const root = parseTree(text);
    if (!root) {
        return [];
    }
    const diagnostics: WhenDiagnostic[] = [];
    const visit = (node: Node): void => {
        if (node.type === 'property' && node.children?.length === 2) {
            const [keyNode, valueNode] = node.children;
            if (
                keyNode.value === 'when' &&
                valueNode.type === 'string' &&
                typeof valueNode.value === 'string'
            ) {
                const message = validateWhenValue(valueNode.value);
                if (message) {
                    diagnostics.push({
                        message,
                        offset: valueNode.offset,
                        length: valueNode.length,
                    });
                }
            }
        }
        node.children?.forEach(visit);
    };
    visit(root);
    return diagnostics;
}

function isKeybindingsModel(model: editor.ITextModel): boolean {
    return model.uri.path.endsWith('/keybindings.json');
}

function refreshMarkers(monaco: Monaco, model: editor.ITextModel): void {
    const markers: editor.IMarkerData[] = validateWhenClauses(
        model.getValue(),
    ).map((d) => {
        const start = model.getPositionAt(d.offset);
        const end = model.getPositionAt(d.offset + d.length);
        return {
            severity: monaco.MarkerSeverity.Warning,
            message: d.message,
            startLineNumber: start.lineNumber,
            startColumn: start.column,
            endLineNumber: end.lineNumber,
            endColumn: end.column,
        };
    });
    monaco.editor.setModelMarkers(model, MARKER_OWNER, markers);
}

/**
 * Validate `when` clauses in every keybindings buffer (current and future) and
 * keep markers in sync as the user edits. Returns a disposable that detaches
 * the listeners and clears the markers.
 */
export function registerKeybindingsDiagnostics(monaco: Monaco): {
    dispose: () => void;
} {
    const disposables: Array<{ dispose: () => void }> = [];
    const watch = (model: editor.ITextModel): void => {
        if (!isKeybindingsModel(model)) {
            return;
        }
        refreshMarkers(monaco, model);
        disposables.push(
            model.onDidChangeContent(() => refreshMarkers(monaco, model)),
        );
    };
    monaco.editor.getModels().forEach(watch);
    disposables.push(monaco.editor.onDidCreateModel(watch));
    return {
        dispose: () => {
            disposables.forEach((d) => d.dispose());
            for (const model of monaco.editor.getModels()) {
                if (isKeybindingsModel(model)) {
                    monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
                }
            }
        },
    };
}
