/**
 * Wires the bootstrap context keys consumed by Phase 2.x `when` clauses:
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

import { contextKeys } from './contextKey';

export type Teardown = () => void;

// VS Code names several focus context keys that map onto Operator's single
// editor; keep them in lock-step so VS Code-authored `when` clauses gate
// correctly. `editorFocused` is Operator's own; the rest are VS Code aliases.
const EDITOR_FOCUS_KEYS = [
    'editorFocused',
    'editorFocus',
    'editorTextFocus',
    'textInputFocus',
    'inputFocus',
] as const;

function setEditorFocus(focused: boolean): void {
    const next: Record<string, boolean> = {};
    for (const key of EDITOR_FOCUS_KEYS) {
        next[key] = focused;
    }
    contextKeys.setMany(next);
}

/** Mirror Monaco focus state into `editorFocused` and its VS Code aliases. */
export function bindEditorFocus(ed: editor.IStandaloneCodeEditor): Teardown {
    setEditorFocus(ed.hasWidgetFocus());
    const focusSub = ed.onDidFocusEditorWidget(() => {
        setEditorFocus(true);
    });
    const blurSub = ed.onDidBlurEditorWidget(() => {
        setEditorFocus(false);
    });
    return () => {
        focusSub.dispose();
        blurSub.dispose();
        setEditorFocus(false);
    };
}

/**
 * Seed the static editor context keys VS Code `when` clauses commonly test.
 * Operator's editor is always editable JavaScript with a definition provider,
 * so these are constant; provider keys we do not satisfy stay unset (falsy),
 * which simply means those bindings never fire.
 */
export function bindEditorContextConstants(): Teardown {
    contextKeys.setMany({
        editorReadonly: false,
        editorHasDefinitionProvider: true,
        editorLangId: 'javascript',
    });
    return () => {
        contextKeys.unset('editorReadonly');
        contextKeys.unset('editorHasDefinitionProvider');
        contextKeys.unset('editorLangId');
    };
}

/**
 * Watch the editor DOM for the suggest / find widgets and mirror their
 * visibility into the context-key service. Returns a teardown.
 *
 * Implementation: a single `MutationObserver` on the editor root,
 * triggered on subtree class changes. We then re-scan for the two
 * widgets we care about — cheap because the widget count is small.
 */
export function bindEditorWidgetVisibility(
    ed: editor.IStandaloneCodeEditor,
): Teardown {
    const root = ed.getDomNode();
    if (!root) return () => {};

    const scan = () => {
        const suggest = root.querySelector('.suggest-widget');
        const find = root.querySelector('.find-widget');
        contextKeys.setMany({
            suggestWidgetVisible: !!(
                suggest && suggest.classList.contains('visible')
            ),
            findWidgetVisible: !!(
                find && find.classList.contains('visible')
            ),
        });
    };
    scan();

    const observer = new MutationObserver(scan);
    observer.observe(root, {
        subtree: true,
        attributes: true,
        attributeFilter: ['class'],
        childList: true,
    });

    return () => {
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
        contextKeys.set('fileExplorerFocused', el.contains(document.activeElement));
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
