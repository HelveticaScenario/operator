import type { editor } from 'monaco-editor';
import electronAPI from '../../electronAPI';
import type { VideoPreviewZone } from '../../types/editor';
import { drawPreview } from '../../video/previewViews';

interface VideoPreviewViewZoneParams {
    editor: editor.IStandaloneCodeEditor;
    zones: VideoPreviewZone[];
    /** Tracked decoration collection addressed by each zone's `decorationIndex`. */
    decorations: editor.IEditorDecorationsCollection | null;
}

export interface VideoPreviewViewZoneHandle {
    /** Removes every zone and stops listening for frames. */
    dispose: () => void;
    /** Moves each zone to its anchor's current line, hiding zones whose anchor is gone. */
    repositionZones: () => void;
}

/** Panel heights in pixels; the monitors need more room than a picture. */
const ZONE_HEIGHT = { image: 112, vectorscope: 152, waveform: 136 } as const;
const PANEL_PADDING = 8;

function resolveLine(
    decorations: editor.IEditorDecorationsCollection | null,
    decorationIndex: number | null,
): number | null {
    if (decorations && decorationIndex !== null) {
        const range = decorations.getRange(decorationIndex);
        if (range && !range.isEmpty()) return range.endLineNumber;
    }
    return null;
}

/**
 * Shows each `$v.preview` as a panel under its call. Frames arrive from the
 * performance window and are drawn into the panel with the matching index.
 */
export function createVideoPreviewViewZones({
    editor,
    zones,
    decorations,
}: VideoPreviewViewZoneParams): VideoPreviewViewZoneHandle {
    const panels = zones.map((zone) => {
        const line = resolveLine(decorations, zone.decorationIndex);
        if (line === null) return null;
        const height = ZONE_HEIGHT[zone.view];

        const canvas = document.createElement('canvas');
        canvas.className = 'video-preview-canvas';
        canvas.style.height = `${height - PANEL_PADDING}px`;
        canvas.style.background = '#000';
        canvas.style.border = '1px solid rgba(255, 255, 255, 0.15)';
        canvas.style.boxSizing = 'border-box';

        const container = document.createElement('div');
        container.className = 'video-preview-view-zone';
        container.style.display = 'flex';
        container.style.alignItems = 'center';
        container.style.height = `${height}px`;
        container.appendChild(canvas);

        const delegate: editor.IViewZone = {
            afterLineNumber: line,
            domNode: container,
            heightInPx: height,
        };
        return {
            canvas,
            decorationIndex: zone.decorationIndex,
            delegate,
            zone,
        };
    });

    const ids: (string | null)[] = [];
    editor.changeViewZones((accessor) => {
        for (const panel of panels) {
            ids.push(panel ? accessor.addZone(panel.delegate) : null);
        }
    });

    const canvasByIndex = new Map<number, HTMLCanvasElement>();
    const viewByIndex = new Map<number, VideoPreviewZone['view']>();
    for (const panel of panels) {
        if (panel === null) continue;
        canvasByIndex.set(panel.zone.index, panel.canvas);
        viewByIndex.set(panel.zone.index, panel.zone.view);
    }
    const stopFrames = electronAPI.video.onPreviewFrame((frame) => {
        const canvas = canvasByIndex.get(frame.index);
        const view = viewByIndex.get(frame.index);
        if (canvas && view) drawPreview(canvas, view, frame);
    });

    const repositionZones = () => {
        const removed: string[] = [];
        let moved = false;
        panels.forEach((panel, i) => {
            const id = ids[i];
            if (panel === null || id === null) return;
            const line = resolveLine(decorations, panel.decorationIndex);
            if (line === null) {
                removed.push(id);
                ids[i] = null;
                canvasByIndex.delete(panel.zone.index);
            } else if (panel.delegate.afterLineNumber !== line) {
                panel.delegate.afterLineNumber = line;
                moved = true;
            }
        });
        if (moved || removed.length > 0) {
            editor.changeViewZones((accessor) => {
                for (const id of removed) accessor.removeZone(id);
                for (const id of ids) {
                    if (id !== null) accessor.layoutZone(id);
                }
            });
        }
    };

    const dispose = () => {
        stopFrames();
        const live = ids.filter((id): id is string => id !== null);
        if (live.length > 0) {
            editor.changeViewZones((accessor) => {
                for (const id of live) accessor.removeZone(id);
            });
        }
        ids.length = 0;
        canvasByIndex.clear();
    };

    return { dispose, repositionZones };
}
