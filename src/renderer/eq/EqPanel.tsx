import React, {
    useCallback,
    useEffect,
    useLayoutEffect,
    useMemo,
    useRef,
    useState,
} from 'react';

import { drawEq, makeScratch } from './eqCanvas';
import { readEqColors } from './eqCanvasLayers';
import { type EqGeometry, makeGeometry } from './eqGeometry';
import type { EqChangePhase } from './eqGestures';
import type { EqSectionChange, EqSectionView } from './eqPanelModel';
import { EqToolbar } from './EqToolbar';
import { useEqGraphGestures } from './useEqGraphGestures';
import type { SpectrumAnalyzer } from './eqSpectrum';
import './EqPanel.css';

export const EQ_GRAPH_HEIGHT = 170;
/** Graph plus toolbar; the view zone reserves exactly this. */
export const EQ_PANEL_HEIGHT = EQ_GRAPH_HEIGHT + 34;

export interface EqPanelProps {
    /** The edited list's sections. */
    sections: EqSectionView[];
    /** Every channel's list in the per-channel form, else null. */
    channelLists: EqSectionView[][] | null;
    /** Index of the edited list in `channelLists`. */
    channel: number;
    /** Code-side section values diverging from the audio, index-aligned. */
    ghosts: (EqSectionView | null)[];
    selected: number | null;
    adaptiveQ: boolean;
    sampleRate: number;
    /** False when the call site cannot be edited: changes are live-only. */
    editable: boolean;
    /** Shown when the code can no longer be updated from the panel. */
    notice: string | null;
    spectrum: SpectrumAnalyzer;
    /** Calls back after each new spectrum frame; returns an unsubscribe. */
    subscribeFrames: (cb: () => void) => () => void;
    onSelect: (index: number | null) => void;
    onChannel: (list: number) => void;
    /** A gesture step (`move`) or its end (`commit`, flushes the code). */
    onChange: (
        index: number,
        change: EqSectionChange,
        phase: EqChangePhase,
        codeOnly: boolean,
    ) => void;
    onAdd: (freqHz: number, gainDb: number) => void;
    onRemove: (index: number) => void;
    /** Add a channel list copying the edited one (splits the shared form). */
    onAddChannel: () => void;
    onRemoveChannel: (list: number) => void;
    /** Write the audio's values over a section's code-only edits (`fields`,
     *  or all when null). */
    onRevert: (index: number, fields: (keyof EqSectionChange)[] | null) => void;
}

export function EqPanel(props: EqPanelProps): React.ReactElement {
    const {
        sections,
        channelLists,
        channel,
        ghosts,
        selected,
        adaptiveQ,
        sampleRate,
        editable,
        notice,
        spectrum,
        subscribeFrames,
        onSelect,
        onChannel,
        onChange,
        onAdd,
        onRemove,
        onAddChannel,
        onRemoveChannel,
        onRevert,
    } = props;

    const rootRef = useRef<HTMLDivElement>(null);
    const wrapRef = useRef<HTMLDivElement>(null);
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const [width, setWidth] = useState(0);
    const [hovered, setHovered] = useState<number | null>(null);
    const [overHandle, setOverHandle] = useState(false);

    const geometry = useMemo<EqGeometry | null>(
        () =>
            width > 0 ? makeGeometry(width, EQ_GRAPH_HEIGHT, sampleRate) : null,
        [width, sampleRate],
    );
    const scratch = useMemo(
        () => (geometry ? makeScratch(geometry) : null),
        [geometry],
    );

    // Everything a redraw reads, so frame callbacks draw current state.
    const background = useMemo(
        () => channelLists?.filter((_, i) => i !== channel) ?? [],
        [channelLists, channel],
    );
    const drawStateRef = useRef({
        adaptiveQ,
        background,
        ghosts,
        hovered,
        sections,
        selected,
    });
    useLayoutEffect(() => {
        drawStateRef.current = {
            adaptiveQ,
            background,
            ghosts,
            hovered,
            sections,
            selected,
        };
    });

    const redraw = useCallback(() => {
        const canvas = canvasRef.current;
        if (!canvas || !geometry || !scratch) {
            return;
        }
        const s = drawStateRef.current;
        drawEq(
            canvas,
            geometry,
            scratch,
            {
                adaptiveQ: s.adaptiveQ,
                background: s.background,
                hovered: s.hovered,
                sampleRate,
                sections: s.sections.map((view, i) => ({
                    ghost: s.ghosts[i] ?? null,
                    view,
                })),
                selected: s.selected,
                spectrum,
            },
            readEqColors(),
        );
    }, [geometry, scratch, sampleRate, spectrum]);

    useEffect(() => {
        const wrap = wrapRef.current;
        if (!wrap) {
            return;
        }
        const observer = new ResizeObserver(() => {
            setWidth(Math.floor(wrap.clientWidth));
        });
        observer.observe(wrap);
        return () => observer.disconnect();
    }, []);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas || width === 0) {
            return;
        }
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.max(1, Math.floor(width * dpr));
        canvas.height = Math.floor(EQ_GRAPH_HEIGHT * dpr);
        redraw();
    }, [width, redraw]);

    useEffect(() => {
        redraw();
    }, [redraw, sections, background, ghosts, selected, hovered, adaptiveQ]);

    useEffect(() => subscribeFrames(redraw), [subscribeFrames, redraw]);

    const gestures = useEqGraphGestures({
        canvasRef,
        editable,
        geometry,
        ghosts,
        hovered,
        onAdd,
        onChange,
        onRevert,
        onSelect,
        rootRef,
        sections,
        selected,
        setHovered,
        setOverHandle,
    });

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (
            (e.key === 'Delete' || e.key === 'Backspace') &&
            selected !== null
        ) {
            e.preventDefault();
            e.stopPropagation();
            onRemove(selected);
        } else if (e.key === 'Escape' && selected !== null) {
            e.stopPropagation();
            onSelect(null);
        }
    };

    return (
        <div
            className="eq-panel"
            ref={rootRef}
            tabIndex={-1}
            onKeyDown={handleKeyDown}
        >
            <div className="eq-panel-graph" ref={wrapRef}>
                <canvas
                    ref={canvasRef}
                    className={
                        overHandle
                            ? 'eq-canvas eq-canvas-handle'
                            : hovered !== null
                              ? 'eq-canvas eq-canvas-node'
                              : 'eq-canvas'
                    }
                    style={{ height: EQ_GRAPH_HEIGHT }}
                    onPointerDown={gestures.onPointerDown}
                    onPointerMove={gestures.onPointerMove}
                    onPointerUp={gestures.onPointerUp}
                    onPointerCancel={gestures.onPointerCancel}
                    onPointerLeave={() => {
                        setHovered(null);
                        setOverHandle(false);
                    }}
                    onDoubleClick={gestures.onDoubleClick}
                    onContextMenu={gestures.onContextMenu}
                />
                <div className="eq-panel-badges">
                    {notice && (
                        <span className="eq-badge eq-badge-warn">{notice}</span>
                    )}
                    {!editable && (
                        <span
                            className="eq-badge"
                            title="This call runs several times (a loop or helper), so the panel cannot write one set of values into the code"
                        >
                            live only
                        </span>
                    )}
                </div>
            </div>
            <EqToolbar
                channel={channel}
                channelLists={channelLists}
                editable={editable}
                ghosts={ghosts}
                sections={sections}
                selected={selected}
                onAdd={onAdd}
                onAddChannel={onAddChannel}
                onChange={onChange}
                onChannel={onChannel}
                onRemove={onRemove}
                onRemoveChannel={onRemoveChannel}
                onRevert={onRevert}
                onSelect={onSelect}
            />
        </div>
    );
}
