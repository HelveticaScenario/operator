import type * as Monaco from 'monaco-editor';
// monaco-editor ships types only for its main entry; allowJs resolves this
// deep ESM file directly, so an ambient declare-module is ignored. The symbol
// is typed `any`; the calls below are trivial.
// @ts-expect-error -- no shipped types for this deep ESM path (monaco-editor#4612)
import { getDefaultHoverDelegate } from 'monaco-editor/esm/vs/base/browser/ui/hover/hoverDelegateFactory.js';

let anchorEditor: Monaco.editor.IStandaloneCodeEditor | undefined;

/**
 * Workaround for microsoft/monaco-editor#4612.
 *
 * Monaco's standalone hover-delegate factory is a process-global singleton:
 * every StandaloneEditor / DiffEditor constructor overwrites it (via
 * setHoverDelegateFactory) with a closure bound to that editor's own,
 * disposable, instantiation service, and the module-level Lazy behind
 * getDefaultHoverDelegate resolves exactly once. When a transient editor — the
 * migration DiffEditor — is the last to set the factory and is then disposed,
 * the Lazy resolves against the dead service and throws "InstantiationService
 * has been disposed" the next time a suggest item or hover renders.
 *
 * This creates one detached, never-rendered, never-disposed editor whose scoped
 * instantiation service therefore lives for the whole session, then resolves
 * the Lazy while that anchor owns the factory. The default hover delegate is
 * permanently bound to the anchor's immortal service, so real editors may
 * overwrite the factory and be disposed freely — including a full
 * unmount/remount of the main editor.
 *
 * Idempotent; safe to call on every monaco load.
 */
export function installHoverDelegateAnchor(monaco: typeof Monaco): void {
    if (anchorEditor) {
        return;
    }
    try {
        // Detached container: the anchor never needs to render. The hover it
        // backs is positioned relative to the real editor's target element.
        const ed = monaco.editor.create(document.createElement('div'));
        // Resolve the Lazy now, while the anchor owns the factory, so it caches
        // a delegate bound to the anchor's immortal instantiation service.
        getDefaultHoverDelegate('element');
        getDefaultHoverDelegate('mouse');
        anchorEditor = ed;
    } catch (err) {
        // Never let this workaround break renderer bootstrap.
        console.warn('Failed to install monaco hover-delegate anchor:', err);
    }
}
