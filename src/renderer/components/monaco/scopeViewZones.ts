import type { ScopeView } from '../../types/editor';
import type { editor } from 'monaco-editor';
import type { Monaco } from '../../hooks/useCustomMonaco';

interface ScopeViewZoneParams {
    editor: editor.IStandaloneCodeEditor;
    monaco: Monaco;
    views: ScopeView[];
    /** Tracked decoration collection addressed by each view's `decorationIndex`. */
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
 * Resolve the afterLineNumber for a scope view zone from its view's
 * decoration index. Returns `null` for anchorless views (null index) and
 * when the decoration range has been deleted (empty/missing), signalling
 * that the view zone should be hidden.
 */
function resolveLineNumber(
    scopeDecorations: editor.IEditorDecorationsCollection | null,
    decorationIndex: number | null,
): number | null {
    if (scopeDecorations && decorationIndex !== null) {
        const range = scopeDecorations.getRange(decorationIndex);
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
    /** Each zone's canvas keys, 1:1 with viewZoneIds, for canvas unregistration */
    const viewCanvasKeys: string[][] = [];
    /** Each zone's decoration index, for re-resolving positions on reposition */
    const viewDecorationIndexes: (number | null)[] = [];
    const scopeCanvasMap = new Map<string, HTMLCanvasElement>();
    let resizeObserver: ResizeObserver | null = null;
    let dprQuery: MediaQueryList | null = null;

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
        viewCanvasKeys.length = 0;
        viewDecorationIndexes.length = 0;

        scopeCanvasMap.forEach((_canvas, key) => {
            onUnregisterScopeCanvas?.(key);
        });
        scopeCanvasMap.clear();

        if (resizeObserver) {
            resizeObserver.disconnect();
            resizeObserver = null;
        }
        if (dprQuery) {
            dprQuery.removeEventListener('change', onDprChange);
            dprQuery = null;
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
    const splitGap = 4;

    const zones = views.map((view) => {
        // A view without a resolvable anchor (null decorationIndex, or a
        // decoration range that has collapsed) has no anchor line in the
        // document, so it gets no zone and no canvas — mirroring the removal
        // path in repositionZones.
        const resolvedLine = resolveLineNumber(
            scopeDecorations,
            view.decorationIndex,
        );
        if (resolvedLine === null) {
            return {
                canvasKeys: [],
                decorationIndex: view.decorationIndex,
                delegate: null,
            };
        }

        // A split view draws one canvas per channel, each registered under its
        // own key so the render loop treats it as an independent scope.
        const canvasSpecs =
            view.split && view.channelKeys.length > 1
                ? view.channelKeys.map((channelKey, i) => ({
                      channelKeys: [channelKey],
                      key: `${view.key}#${i}`,
                  }))
                : [{ channelKeys: view.channelKeys, key: view.key }];
        const stacked = view.split === 'vertical';
        const count = canvasSpecs.length;
        const zoneHeight = stacked
            ? count * scopeHeight + (count - 1) * splitGap
            : scopeHeight;
        const seedWidth = stacked
            ? layoutInfo.contentWidth
            : (layoutInfo.contentWidth - (count - 1) * splitGap) / count;

        const container = document.createElement('div');
        container.className = 'scope-view-zone';
        container.style.height = `${zoneHeight}px`;
        container.style.width = '100%';

        // Monaco owns the zone node's `display` (it toggles it to hide zones),
        // so the flex layout lives on an inner element.
        const layout = document.createElement('div');
        layout.style.height = '100%';
        layout.style.display = 'flex';
        layout.style.flexDirection = stacked ? 'column' : 'row';
        layout.style.gap = `${splitGap}px`;
        container.appendChild(layout);

        for (const spec of canvasSpecs) {
            const canvas = document.createElement('canvas');
            if (stacked || count === 1) {
                canvas.style.width = '100%';
            } else {
                canvas.style.flex = '1 1 0';
                canvas.style.minWidth = '0';
            }
            canvas.style.height = `${scopeHeight}px`;
            canvas.dataset.scopeKey = spec.key;
            canvas.dataset.scopeRangeMin = String(view.range[0]);
            canvas.dataset.scopeRangeMax = String(view.range[1]);
            canvas.dataset.scopeChannelKeys = JSON.stringify(spec.channelKeys);

            // Seed the backing store from the editor content width; the canvas
            // is still detached here so its own clientWidth is 0.
            // resizeCanvases() below re-syncs it to the real display size once
            // attached.
            canvas.width = Math.max(1, Math.floor(seedWidth * dpr));
            canvas.height = Math.max(1, Math.floor(scopeHeight * dpr));

            layout.appendChild(canvas);

            scopeCanvasMap.set(spec.key, canvas);
            onRegisterScopeCanvas?.(spec.key, canvas);
        }

        const delegate: editor.IViewZone = {
            afterLineNumber: resolvedLine,
            domNode: container,
            heightInPx: zoneHeight,
            marginDomNode: undefined,
        };

        return {
            canvasKeys: canvasSpecs.map((spec) => spec.key),
            decorationIndex: view.decorationIndex,
            delegate,
        };
    });

    editor.changeViewZones((accessor) => {
        for (const { canvasKeys, decorationIndex, delegate } of zones) {
            viewZoneDelegates.push(delegate);
            viewZoneIds.push(delegate ? accessor.addZone(delegate) : null);
            viewCanvasKeys.push(canvasKeys);
            viewDecorationIndexes.push(decorationIndex);
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

            const resolvedLine = resolveLineNumber(
                scopeDecorations,
                viewDecorationIndexes[i],
            );

            if (resolvedLine === null) {
                // Decoration was deleted — remove the view zone entirely and
                // Unregister the canvas so the renderer stops painting it.
                idsToRemove.push(viewZoneIds[i]!);
                viewZoneIds[i] = null;
                viewZoneDelegates[i] = null;

                for (const key of viewCanvasKeys[i]) {
                    if (scopeCanvasMap.has(key)) {
                        onUnregisterScopeCanvas?.(key);
                        scopeCanvasMap.delete(key);
                    }
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

    // Track every display-size change of the canvases themselves (editor
    // layout, window resize, panels squeezing the editor). Observation also
    // fires once on attach, covering zones that mount after this call.
    resizeObserver = new ResizeObserver(resizeCanvases);
    scopeCanvasMap.forEach((canvas) => {
        resizeObserver!.observe(canvas);
    });

    // A pure devicePixelRatio change — dragging the window between displays of
    // different density — leaves the canvases' content box unchanged, so the
    // ResizeObserver never fires. Watch the ratio directly; each media query
    // matches a single dpr, so re-arm a fresh one after every change.
    function onDprChange() {
        resizeCanvases();
        watchDpr();
    }
    function watchDpr() {
        if (
            typeof window === 'undefined' ||
            typeof window.matchMedia !== 'function'
        ) {
            return;
        }
        const current = window.devicePixelRatio || 1;
        dprQuery = window.matchMedia(`(resolution: ${current}dppx)`);
        dprQuery.addEventListener('change', onDprChange, { once: true });
    }
    watchDpr();

    return { dispose, repositionZones };
}
