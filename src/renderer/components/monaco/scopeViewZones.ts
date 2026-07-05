import type { ScopeView } from '../../types/editor';
import type { editor } from 'monaco-editor';
import type { Monaco } from '../../hooks/useCustomMonaco';

interface ScopeViewZoneParams {
    editor: editor.IStandaloneCodeEditor;
    monaco: Monaco;
    views: ScopeView[];
    /** Tracked decoration collection whose ranges correspond 1:1 with `views`. */
    scopeDecorations: editor.IEditorDecorationsCollection | null;
    onRegisterScopeCanvas?: (key: string, canvas: HTMLCanvasElement) => void;
    onUnregisterScopeCanvas?: (key: string) => void;
}

export interface ScopeViewZoneHandle {
    /** Tear down all view zones, canvases, and listeners. */
    dispose: () => void;
    /** Re-read positions from the tracked decoration collection and call
     *  layoutZone for any view zone whose end-line has shifted. */
    repositionZones: () => void;
}

/**
 * Resolve the afterLineNumber for a scope view zone.
 * Reads from the tracked decoration collection when available.
 * Returns `null` if the decoration range has been deleted (empty/missing),
 * signalling that the view zone should be hidden.
 */
function resolveLineNumber(
    scopeDecorations: editor.IEditorDecorationsCollection | null,
    index: number,
): number | null {
    if (scopeDecorations) {
        const range = scopeDecorations.getRange(index);
        if (range && !range.isEmpty()) {
            return range.endLineNumber;
        }
    }
    return null;
}

export function createScopeViewZones({
    editor,
    monaco: _monaco,
    views,
    scopeDecorations,
    onRegisterScopeCanvas,
    onUnregisterScopeCanvas,
}: ScopeViewZoneParams): ScopeViewZoneHandle {
    const viewZoneIds: (string | null)[] = [];
    /** Retained delegate objects so we can mutate afterLineNumber + layoutZone */
    const viewZoneDelegates: (editor.IViewZone | null)[] = [];
    /** Scope keys corresponding 1:1 with viewZoneIds, for canvas unregistration */
    const viewKeys: string[] = [];
    const scopeCanvasMap = new Map<string, HTMLCanvasElement>();
    let layoutListener: ReturnType<
        editor.IStandaloneCodeEditor['onDidLayoutChange']
    > | null = null;

    const dispose = () => {
        const idsToRemove = viewZoneIds.filter(
            (id): id is string => id !== null,
        );
        if (idsToRemove.length > 0) {
            editor.changeViewZones((accessor) => {
                for (const id of idsToRemove) {
                    accessor.removeZone(id);
                }
            });
        }
        viewZoneIds.length = 0;
        viewZoneDelegates.length = 0;
        viewKeys.length = 0;

        scopeCanvasMap.forEach((_canvas, key) => {
            onUnregisterScopeCanvas?.(key);
        });
        scopeCanvasMap.clear();

        if (layoutListener) {
            layoutListener.dispose();
            layoutListener = null;
        }
    };

    dispose();

    const noopHandle: ScopeViewZoneHandle = {
        dispose,
        repositionZones: () => {},
    };

    if (views.length === 0) {
        return noopHandle;
    }

    const dpr =
        typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
    const layoutInfo = editor.getLayoutInfo();
    const scopeHeight = 80; // Increased height for legend and stats

    const zones = views.map((view, index) => {
        // A view without a resolvable decoration range has no anchor line in
        // the document, so it gets no zone and no canvas — mirroring the
        // removal path in repositionZones. The null placeholders keep the
        // per-index arrays aligned 1:1 with `views`.
        const resolvedLine = resolveLineNumber(scopeDecorations, index);
        if (resolvedLine === null) {
            return { delegate: null, key: view.key };
        }

        const container = document.createElement('div');
        container.className = 'scope-view-zone';
        container.style.height = `${scopeHeight}px`;
        container.style.width = '100%';
        container.style.display = 'flex';

        const canvas = document.createElement('canvas');
        canvas.style.width = '100%';
        canvas.style.height = `${scopeHeight}px`;
        canvas.dataset.scopeKey = view.key;
        canvas.dataset.scopeRangeMin = String(view.range[0]);
        canvas.dataset.scopeRangeMax = String(view.range[1]);
        canvas.dataset.scopeChannelKeys = JSON.stringify(view.channelKeys);

        // Seed the backing store from the editor content width; the canvas is
        // still detached here so its own clientWidth is 0. resizeCanvases()
        // below re-syncs it to the real display size once attached.
        canvas.width = Math.max(1, Math.floor(layoutInfo.contentWidth * dpr));
        canvas.height = Math.max(1, Math.floor(scopeHeight * dpr));

        container.appendChild(canvas);

        scopeCanvasMap.set(view.key, canvas);
        onRegisterScopeCanvas?.(view.key, canvas);

        const delegate: editor.IViewZone = {
            afterLineNumber: resolvedLine,
            domNode: container,
            heightInPx: scopeHeight,
            marginDomNode: undefined,
        };

        return { delegate, key: view.key };
    });

    editor.changeViewZones((accessor) => {
        for (const { delegate, key } of zones) {
            viewZoneDelegates.push(delegate);
            viewZoneIds.push(delegate ? accessor.addZone(delegate) : null);
            viewKeys.push(key);
        }
    });

    const repositionZones = () => {
        if (!scopeDecorations || viewZoneIds.length === 0) {
            return;
        }

        let needsLayout = false;
        const idsToRemove: string[] = [];

        for (let i = 0; i < viewZoneDelegates.length; i++) {
            if (viewZoneIds[i] === null) {
                continue;
            } // Already removed

            const resolvedLine = resolveLineNumber(scopeDecorations, i);

            if (resolvedLine === null) {
                // Decoration was deleted — remove the view zone entirely and
                // Unregister the canvas so the renderer stops painting it.
                idsToRemove.push(viewZoneIds[i]!);
                viewZoneIds[i] = null;
                viewZoneDelegates[i] = null;

                const key = viewKeys[i];
                if (scopeCanvasMap.has(key)) {
                    onUnregisterScopeCanvas?.(key);
                    scopeCanvasMap.delete(key);
                }

                needsLayout = true;
                continue;
            }

            const delegate = viewZoneDelegates[i]!;
            if (delegate.afterLineNumber !== resolvedLine) {
                delegate.afterLineNumber = resolvedLine;
                needsLayout = true;
            }
        }

        if (needsLayout) {
            editor.changeViewZones((accessor) => {
                for (const id of idsToRemove) {
                    accessor.removeZone(id);
                }
                for (let i = 0; i < viewZoneIds.length; i++) {
                    if (viewZoneIds[i] !== null) {
                        accessor.layoutZone(viewZoneIds[i]!);
                    }
                }
            });
        }
    };

    const resizeCanvases = () => {
        const nextDpr =
            typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
        scopeCanvasMap.forEach((canvas) => {
            // Match the backing store to the canvas's actual displayed size so
            // the waveform keeps its aspect ratio. Reassigning canvas.width
            // clears the canvas, so only do it when a dimension truly changed —
            // otherwise every scroll-driven layout event wipes the last frame.
            const displayWidth = canvas.clientWidth || canvas.width / nextDpr;
            const nextWidth = Math.max(1, Math.floor(displayWidth * nextDpr));
            const nextHeight = Math.max(1, Math.floor(scopeHeight * nextDpr));
            if (canvas.width !== nextWidth) {
                canvas.width = nextWidth;
            }
            if (canvas.height !== nextHeight) {
                canvas.height = nextHeight;
            }
        });
    };

    // Sync once now that the zones are attached and have a real display width.
    resizeCanvases();

    layoutListener = editor.onDidLayoutChange(resizeCanvases);

    return { dispose, repositionZones };
}
