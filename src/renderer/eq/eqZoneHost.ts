/**
 * Places each panel in a Monaco view zone after its call's last line,
 * following edits, hiding while another model is shown, and keeping the
 * editor's own mouse handling and context menu off the panel.
 */
import { createRoot } from 'react-dom/client';
import type { editor } from 'monaco-editor';

import { EQ_PANEL_HEIGHT } from './EqPanel';
import type { EqPanelSetup } from './eqPanelSetup';
import type { PanelRuntime, Zone } from './eqPanelRuntime';

/** The after-line for a panel's zone, or null when it has no anchor. */
function zoneLine(
    ed: editor.IStandaloneCodeEditor,
    setup: EqPanelSetup | null,
    panel: PanelRuntime,
): number | null {
    if (
        !setup?.model ||
        setup.model.isDisposed() ||
        ed.getModel() !== setup.model
    ) {
        return null;
    }
    const range = setup.decorations?.getRange(panel.index);
    return range && !range.isEmpty() ? range.endLineNumber : null;
}

function createZoneDom(): HTMLElement {
    const dom = document.createElement('div');
    dom.className = 'eq-view-zone';
    // Above Monaco's .view-lines layer, or clicks move the cursor.
    Object.assign(dom.style, {
        pointerEvents: 'auto',
        position: 'relative',
        zIndex: '10',
    });
    for (const type of ['mousedown', 'pointerdown']) {
        dom.addEventListener(type, (e) => e.stopPropagation());
    }
    // The editor's context menu never opens over the panel; the panel's own
    // right-click gestures are React handlers on this same element, so they
    // still run.
    dom.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
    });
    return dom;
}

function unmountZone(zone: Zone): void {
    // Unmounting synchronously inside a React commit warns; defer it.
    queueMicrotask(() => zone.root.unmount());
}

/**
 * Add, move, or remove zones to match the anchors. Returns the panels that
 * gained a zone (they need a first render), or null when nothing changed.
 */
export function syncZones(
    ed: editor.IStandaloneCodeEditor,
    setup: EqPanelSetup | null,
    panels: Iterable<PanelRuntime>,
): PanelRuntime[] | null {
    const wanted = [...panels].map((panel) => ({
        line: zoneLine(ed, setup, panel),
        panel,
    }));
    const changed = wanted.some(({ line, panel }) =>
        line === null
            ? panel.zone !== null
            : panel.zone?.delegate.afterLineNumber !== line,
    );
    if (!changed) {
        return null;
    }
    const added: PanelRuntime[] = [];
    ed.changeViewZones((accessor) => {
        for (const { line, panel } of wanted) {
            if (line === null) {
                if (panel.zone) {
                    accessor.removeZone(panel.zone.id);
                    unmountZone(panel.zone);
                    panel.zone = null;
                }
                continue;
            }
            if (panel.zone) {
                if (panel.zone.delegate.afterLineNumber !== line) {
                    panel.zone.delegate.afterLineNumber = line;
                    accessor.layoutZone(panel.zone.id);
                }
                continue;
            }
            const dom = createZoneDom();
            const delegate: editor.IViewZone = {
                afterLineNumber: line,
                domNode: dom,
                heightInPx: EQ_PANEL_HEIGHT,
                suppressMouseDown: true,
            };
            panel.zone = {
                delegate,
                id: accessor.addZone(delegate),
                root: createRoot(dom),
            };
            added.push(panel);
        }
    });
    if (added.length > 0) {
        sizeZones(ed, added);
    }
    return added;
}

/**
 * Pin each zone to the visible text area: a view zone otherwise spans the
 * widest line, pushing the panel's right side out of view.
 */
export function sizeZones(
    ed: editor.IStandaloneCodeEditor,
    panels: Iterable<PanelRuntime>,
): void {
    const layout = ed.getLayoutInfo();
    const width = Math.max(
        0,
        layout.width - layout.contentLeft - layout.verticalScrollbarWidth,
    );
    for (const panel of panels) {
        if (panel.zone) {
            panel.zone.delegate.domNode.style.width = `${width}px`;
        }
    }
}

export function removeZone(
    ed: editor.IStandaloneCodeEditor | null,
    panel: PanelRuntime,
): void {
    if (!panel.zone) {
        return;
    }
    const zone = panel.zone;
    panel.zone = null;
    try {
        ed?.changeViewZones((accessor) => accessor.removeZone(zone.id));
    } catch {
        // The editor is already disposed, taking its zones with it.
    }
    unmountZone(zone);
}
