/**
 * Wires the bootstrap context keys consumed by `when` clauses:
 *   - editorFocused          (Monaco editor widget has focus)
 *   - suggestWidgetVisible   (Monaco suggest widget on screen)
 *   - findWidgetVisible      (Monaco find widget on screen)
 *   - inSettingsModal        (settings modal mounted)
 *   - fileExplorerFocused    (file explorer subtree has focus)
 *
 * Surfaces import the helper they need and call it from their existing
 * mount / focus effects. Each helper returns a teardown function.
 *
 * Suggest- and find-widget visibility is mirrored from Monaco DOM
 * classes (`.suggest-widget.visible`, `.find-widget.visible`). Monaco's
 * internal `IContextKeyService` is not part of the public standalone
 * API, so an observer is the supported way to read these flags.
 */

import type { editor } from 'monaco-editor';

import { contextKeys, type ContextKeyValue } from './contextKey';

export type Teardown = () => void;

// VS Code names several focus context keys that map onto Operator's single
// editor; keep them in lock-step so VS Code-authored `when` clauses gate
// correctly. `editorFocused` is Operator's own; the rest are VS Code aliases.
// Widget focus spans the editor and its peripheral inputs (find / suggest);
// `editorTextFocus` is narrower — true only while the main text input holds
// focus, so it drops to false when, e.g., the find box is focused.
const WIDGET_FOCUS_KEYS = [
    'editorFocused',
    'editorFocus',
    'textInputFocus',
    'inputFocus',
] as const;

function setWidgetFocus(focused: boolean): void {
    const next: Record<string, boolean> = {};
    for (const key of WIDGET_FOCUS_KEYS) {
        next[key] = focused;
    }
    contextKeys.setMany(next);
}

/** Mirror Monaco focus state into the editor focus keys and VS Code aliases. */
export function bindEditorFocus(ed: editor.IStandaloneCodeEditor): Teardown {
    setWidgetFocus(ed.hasWidgetFocus());
    contextKeys.set('editorTextFocus', ed.hasTextFocus());
    const subs = [
        ed.onDidFocusEditorWidget(() => setWidgetFocus(true)),
        ed.onDidBlurEditorWidget(() => setWidgetFocus(false)),
        ed.onDidFocusEditorText(() => contextKeys.set('editorTextFocus', true)),
        ed.onDidBlurEditorText(() => contextKeys.set('editorTextFocus', false)),
    ];
    return () => {
        for (const sub of subs) {
            sub.dispose();
        }
        setWidgetFocus(false);
        contextKeys.set('editorTextFocus', false);
    };
}

// Provider context keys that monaco-editor's bundled TypeScript/JavaScript
// language service registers by default (its default modeConfiguration enables
// completion, signatureHelp, definitions, references, documentSymbols, rename,
// codeActions, and range formatting). They are live while a js/ts model is
// active and absent for other languages (e.g. the keybindings.json buffer), so
// they track the active model's language rather than being constant.
const JS_TS_PROVIDER_KEYS = [
    'editorHasDefinitionProvider',
    'editorHasReferenceProvider',
    'editorHasDocumentSymbolProvider',
    'editorHasCompletionItemProvider',
    'editorHasSignatureHelpProvider',
    'editorHasCodeActionsProvider',
    'editorHasRenameProvider',
    'editorHasDocumentSelectionFormattingProvider',
] as const;

/**
 * Mirror the editor context keys that VS Code `when` clauses commonly test:
 *   - editorReadonly  — always false (Operator's buffers are editable)
 *   - foldingEnabled  — reflects the editor's `folding` option
 *   - editorLangId    — the active model's language id
 *   - editorHas<X>Provider — true while a js/ts model is active (see above)
 *
 * The provider/language keys track the model so they stay accurate as buffers
 * (the js patch, the keybindings.json editor) swap in and out of the one editor.
 */
export function bindEditorContextConstants(
    ed: editor.IStandaloneCodeEditor,
): Teardown {
    // `folding` is an editor-construction option, not per-model. Monaco's
    // default is on, so treat anything but an explicit false as enabled.
    contextKeys.set('foldingEnabled', ed.getRawOptions().folding !== false);
    contextKeys.set('editorReadonly', false);

    const applyLanguage = () => {
        const langId = ed.getModel()?.getLanguageId() ?? 'plaintext';
        const isJsTs = langId === 'javascript' || langId === 'typescript';
        const next: Record<string, ContextKeyValue> = { editorLangId: langId };
        for (const key of JS_TS_PROVIDER_KEYS) {
            next[key] = isJsTs;
        }
        contextKeys.setMany(next);
    };
    applyLanguage();
    const subs = [
        ed.onDidChangeModel(applyLanguage),
        ed.onDidChangeModelLanguage(applyLanguage),
    ];

    return () => {
        for (const sub of subs) {
            sub.dispose();
        }
        contextKeys.unset('foldingEnabled');
        contextKeys.unset('editorReadonly');
        contextKeys.unset('editorLangId');
        for (const key of JS_TS_PROVIDER_KEYS) {
            contextKeys.unset(key);
        }
    };
}

/**
 * Watch the editor DOM for the suggest / find widgets and mirror their
 * visibility into the context-key service. Returns a teardown.
 *
 * Monaco mutates this subtree on every keystroke and scroll, so the observer
 * only schedules a rescan; the scan itself (two `querySelector`s) runs at most
 * once per animation frame regardless of how many mutations land.
 */
export function bindEditorWidgetVisibility(
    ed: editor.IStandaloneCodeEditor,
): Teardown {
    const root = ed.getDomNode();
    if (!root) return () => {};

    let raf: number | null = null;
    const scan = () => {
        raf = null;
        const suggest = root.querySelector('.suggest-widget');
        const find = root.querySelector('.find-widget');
        contextKeys.setMany({
            suggestWidgetVisible: !!(
                suggest && suggest.classList.contains('visible')
            ),
            findWidgetVisible: !!(find && find.classList.contains('visible')),
        });
    };
    const schedule = () => {
        if (raf != null) return;
        raf = requestAnimationFrame(scan);
    };
    scan();

    const observer = new MutationObserver(schedule);
    observer.observe(root, {
        subtree: true,
        attributes: true,
        attributeFilter: ['class'],
        childList: true,
    });

    return () => {
        if (raf != null) cancelAnimationFrame(raf);
        observer.disconnect();
        contextKeys.setMany({
            suggestWidgetVisible: false,
            findWidgetVisible: false,
        });
    };
}

/** Set `inSettingsModal` while the modal is open. */
export function bindSettingsModal(isOpen: boolean): Teardown {
    contextKeys.set('inSettingsModal', isOpen);
    return () => {
        contextKeys.set('inSettingsModal', false);
    };
}

/**
 * Mirror focus state of the file explorer subtree into
 * `fileExplorerFocused`. Uses bubbling `focusin` / `focusout` so any
 * descendant input gaining focus counts as the tree being focused.
 *
 * `focusout` fires before `focusin` during focus transitions inside the
 * same subtree, so we debounce with `requestAnimationFrame` to avoid
 * flashing the key to `false` mid-tab-cycle.
 */
export function bindFileExplorerFocus(el: HTMLElement): Teardown {
    let raf: number | null = null;
    const apply = () => {
        raf = null;
        contextKeys.set(
            'fileExplorerFocused',
            el.contains(document.activeElement),
        );
    };
    const schedule = () => {
        if (raf != null) return;
        raf = requestAnimationFrame(apply);
    };

    el.addEventListener('focusin', schedule);
    el.addEventListener('focusout', schedule);
    apply();

    return () => {
        if (raf != null) cancelAnimationFrame(raf);
        el.removeEventListener('focusin', schedule);
        el.removeEventListener('focusout', schedule);
        contextKeys.set('fileExplorerFocused', false);
    };
}
