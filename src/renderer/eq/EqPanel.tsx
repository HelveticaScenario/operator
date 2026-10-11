import React, {
    useCallback,
    useEffect,
    useLayoutEffect,
    useMemo,
    useRef,
    useState,
} from 'react';

import {
    DEFAULT_Q,
    type EqSectionType,
    EQ_SECTION_TYPES,
    MAX_EQ_CHANNEL_LISTS,
    MAX_EQ_SECTIONS,
    formatGain,
    formatHz,
    typeHasGain,
} from './eqMath';
import type { EqSectionChange, EqSectionView } from './eqPanelModel';
import {
    type EqGeometry,
    SECTION_COLORS,
    drawEq,
    hitTestNode,
    hitTestQHandle,
    makeGeometry,
    octavesToQ,
    makeScratch,
    nodePosition,
    readEqColors,
    xToFreq,
    yToGain,
} from './eqCanvas';
import type { SpectrumAnalyzer } from './eqSpectrum';
import './EqPanel.css';

export const EQ_GRAPH_HEIGHT = 170;
/** Graph plus toolbar; the view zone reserves exactly this. */
export const EQ_PANEL_HEIGHT = EQ_GRAPH_HEIGHT + 34;

/** Scroll-to-Q sensitivity: Q multiplies by e^(−deltaY·k). */
const WHEEL_Q_PER_PIXEL = 0.0025;
/** Fine-adjust drag scale while Shift is held. */
const FINE_SCALE = 0.1;
/** Idle time after the last wheel step before the edit is committed. */
const WHEEL_COMMIT_MS = 300;
/** Readout scrub sensitivities; right and up raise the value. */
const SCRUB_FREQ_PER_PIXEL = 0.005;
const SCRUB_GAIN_DB_PER_PIXEL = 0.1;
const SCRUB_Q_PER_PIXEL = 0.01;

/**
 * Ctrl/Cmd gestures edit the code only, leaving the running audio as is
 * until the next patch run; the control then shows a ghost of the code-side
 * value. On macOS Ctrl+click arrives as a contextmenu event, so the type
 * buttons detect the code-only gesture there too.
 */
function isCodeOnly(e: { ctrlKey: boolean; metaKey: boolean }): boolean {
    return e.ctrlKey || e.metaKey;
}

export type EqChangePhase = 'move' | 'commit';

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

const TYPE_LABELS: Record<EqSectionType, string> = {
    highShelf: 'High shelf',
    highpass: 'Highpass',
    lowShelf: 'Low shelf',
    lowpass: 'Lowpass',
    notch: 'Notch',
    peak: 'Peak',
};

const TYPE_GLYPHS: Record<EqSectionType, string> = {
    highShelf: 'M1 10 H6 C9 10 10 4 13 4 H19',
    highpass: 'M3 12 C5 6 7 5 10 5 H19',
    lowShelf: 'M1 4 H6 C9 4 10 10 13 10 H19',
    lowpass: 'M1 5 H10 C13 5 15 6 17 12',
    notch: 'M1 5 H7 L10 12 L13 5 H19',
    peak: 'M1 10 C6 10 7 3 10 3 C13 3 14 10 19 10',
};

interface DragState {
    /** Moving the node, or one of its Q handles. */
    mode: 'node' | 'q';
    index: number;
    pointerId: number;
    codeOnly: boolean;
    downX: number;
    downY: number;
    nodeX: number;
    nodeY: number;
    moveFreq: boolean;
    moveGain: boolean;
}

/** Marks a readout whose value follows a modulating signal. */
const DRIVEN_MARK = (
    <span className="eq-driven" title="Modulated: follows its signal">
        {' ∿'}
    </span>
);

/** How one readout shows, scrubs, and parses its value. */
interface ReadoutField {
    title: string;
    format: (v: number) => string;
    /** The value after a scrub of `pixels` from `start`. */
    scrub: (start: number, pixels: number) => number;
    /** Text the type-in box starts with. */
    editText: (v: number) => string;
    /** A typed value, or null when unparsable. */
    parse: (text: string) => number | null;
}

const FREQ_FIELD: ReadoutField = {
    editText: (v) => String(Math.round(v)),
    format: formatHz,
    parse: (text) => {
        const m = /^\s*(\d*\.?\d+)\s*(k)?\s*(hz)?\s*$/i.exec(text);
        if (!m) {
            return null;
        }
        const hz = parseFloat(m[1]) * (m[2] ? 1000 : 1);
        return hz > 0 ? hz : null;
    },
    scrub: (start, pixels) => start * Math.exp(pixels * SCRUB_FREQ_PER_PIXEL),
    title: 'Frequency',
};

const GAIN_FIELD: ReadoutField = {
    editText: (v) => v.toFixed(1),
    format: formatGain,
    parse: (text) => {
        const v = parseFloat(text.replace(/db/i, ''));
        return Number.isFinite(v) ? v : null;
    },
    scrub: (start, pixels) => start + pixels * SCRUB_GAIN_DB_PER_PIXEL,
    title: 'Gain',
};

const Q_FIELD: ReadoutField = {
    editText: (v) => v.toFixed(2),
    format: (v) => `Q ${v.toFixed(2)}`,
    parse: (text) => {
        const v = parseFloat(text);
        return Number.isFinite(v) && v > 0 ? v : null;
    },
    scrub: (start, pixels) => start * Math.exp(pixels * SCRUB_Q_PER_PIXEL),
    title: 'Q',
};

/**
 * A section value: drag it (right/up raises it, Shift for fine) or
 * double-click to type one. A Ctrl/Cmd drag edits the code only, and a
 * code-side value that differs from the audio shows beside it. Read-only
 * while the param is signal-driven.
 */
function Readout({
    field,
    value,
    ghost,
    driven,
    editable,
    onChange,
    onReset,
    onRevert,
}: {
    field: ReadoutField;
    value: number | null;
    /** The code-side value, when it differs from the audio. */
    ghost: number | null;
    driven: boolean;
    editable: boolean;
    /** `v` null ends the gesture without changing the value. */
    onChange: (
        v: number | null,
        phase: EqChangePhase,
        codeOnly: boolean,
    ) => void;
    /** Right-click: back to the param's default; absent when it has none. */
    onReset?: () => void;
    /** Ctrl/Cmd right-click: write the audio's value over the code's. */
    onRevert: () => void;
}): React.ReactElement {
    const [typing, setTyping] = useState(false);
    const scrubRef = useRef<{
        pointerId: number;
        x: number;
        y: number;
        start: number;
        codeOnly: boolean;
    } | null>(null);

    if (driven || value === null) {
        return (
            <span title={field.title}>
                {value === null ? '—' : field.format(value)}
                {driven && DRIVEN_MARK}
            </span>
        );
    }
    if (typing) {
        return (
            <input
                className="eq-value-input"
                autoFocus
                defaultValue={field.editText(value)}
                onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === 'Enter') {
                        const v = field.parse(e.currentTarget.value);
                        if (v !== null) {
                            onChange(v, 'commit', false);
                        }
                        setTyping(false);
                    } else if (e.key === 'Escape') {
                        setTyping(false);
                    }
                }}
                onBlur={() => setTyping(false)}
            />
        );
    }
    const showGhost = ghost !== null && ghost !== value;
    return (
        <span
            className="eq-scrub"
            title={`${field.title}: drag to change, double-click to type${onReset ? ', right-click to reset' : ''} (⌃/⌘-drag: edit code only; ⌃/⌘ right-click: revert the code to the audio)`}
            onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                if (isCodeOnly(e)) {
                    if (showGhost) {
                        onRevert();
                    }
                } else {
                    onReset?.();
                }
            }}
            onPointerDown={(e) => {
                if (e.button !== 0 || (isCodeOnly(e) && !editable)) {
                    return;
                }
                e.stopPropagation();
                e.currentTarget.setPointerCapture(e.pointerId);
                const codeOnly = isCodeOnly(e);
                scrubRef.current = {
                    codeOnly,
                    pointerId: e.pointerId,
                    start: codeOnly ? (ghost ?? value) : value,
                    x: e.clientX,
                    y: e.clientY,
                };
            }}
            onPointerMove={(e) => {
                const s = scrubRef.current;
                if (!s || s.pointerId !== e.pointerId) {
                    return;
                }
                const pixels =
                    (e.clientX - s.x - (e.clientY - s.y)) *
                    (e.shiftKey ? FINE_SCALE : 1);
                onChange(field.scrub(s.start, pixels), 'move', s.codeOnly);
            }}
            onPointerUp={(e) => {
                const s = scrubRef.current;
                if (!s || s.pointerId !== e.pointerId) {
                    return;
                }
                scrubRef.current = null;
                onChange(null, 'commit', s.codeOnly);
            }}
            onDoubleClick={() => setTyping(true)}
        >
            {field.format(value)}
            {showGhost && (
                <span className="eq-ghost-value" title="Value in the code">
                    {' ⇢ '}
                    {field.format(ghost)}
                </span>
            )}
        </span>
    );
}

/** L/R for a stereo pair of lists, channel numbers otherwise. */
function channelLabel(index: number, count: number): string {
    if (count === 2) {
        return index === 0 ? 'L' : 'R';
    }
    return String(index + 1);
}

/** The values a gesture starts from: the ghost for code-only gestures. */
function gestureBase(
    live: EqSectionView,
    ghost: EqSectionView | null,
    codeOnly: boolean,
): EqSectionView {
    return codeOnly && ghost ? ghost : live;
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
    const dragRef = useRef<DragState | null>(null);
    const wheelTimerRef = useRef<number | null>(null);

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

    const localPoint = (e: { clientX: number; clientY: number }) => {
        const rect = canvasRef.current!.getBoundingClientRect();
        return { x: e.clientX - rect.left, y: e.clientY - rect.top };
    };

    const handlePointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
        e.stopPropagation();
        if (!geometry || e.button !== 0) {
            return;
        }
        rootRef.current?.focus({ preventScroll: true });
        const { x, y } = localPoint(e);
        const codeOnly = isCodeOnly(e);
        if (codeOnly && !editable) {
            return;
        }
        if (selected !== null && sections[selected]) {
            const base = gestureBase(
                sections[selected],
                ghosts[selected] ?? null,
                codeOnly,
            );
            if (!base.driven.q && hitTestQHandle(geometry, base, x, y)) {
                e.currentTarget.setPointerCapture(e.pointerId);
                dragRef.current = {
                    codeOnly,
                    downX: x,
                    downY: y,
                    index: selected,
                    mode: 'q',
                    moveFreq: false,
                    moveGain: false,
                    nodeX: 0,
                    nodeY: 0,
                    pointerId: e.pointerId,
                };
                return;
            }
        }
        const index = hitTestNode(geometry, sections, x, y);
        onSelect(index);
        if (index === null) {
            return;
        }
        const base = gestureBase(
            sections[index],
            ghosts[index] ?? null,
            codeOnly,
        );
        const pos = nodePosition(geometry, base);
        if (!pos) {
            return;
        }
        e.currentTarget.setPointerCapture(e.pointerId);
        dragRef.current = {
            codeOnly,
            downX: x,
            downY: y,
            index,
            mode: 'node',
            moveFreq: !base.driven.freq && base.freqHz !== null,
            moveGain:
                typeHasGain(base.type) &&
                !base.driven.gain &&
                base.gainDb !== null,
            nodeX: pos.x,
            nodeY: pos.y,
            pointerId: e.pointerId,
        };
    };

    const handlePointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
        if (!geometry) {
            return;
        }
        const { x, y } = localPoint(e);
        const drag = dragRef.current;
        if (!drag || drag.pointerId !== e.pointerId) {
            const hit = hitTestNode(geometry, sections, x, y);
            if (hit !== hovered) {
                setHovered(hit);
            }
            const sel = selected !== null ? sections[selected] : undefined;
            setOverHandle(
                sel !== undefined &&
                    !sel.driven.q &&
                    hitTestQHandle(geometry, sel, x, y),
            );
            return;
        }
        if (drag.mode === 'q') {
            const base = gestureBase(
                sections[drag.index],
                ghosts[drag.index] ?? null,
                drag.codeOnly,
            );
            if (base.freqHz !== null) {
                const octaves =
                    2 * Math.abs(Math.log2(xToFreq(geometry, x) / base.freqHz));
                onChange(
                    drag.index,
                    { q: octavesToQ(Math.max(octaves, 0.01)) },
                    'move',
                    drag.codeOnly,
                );
            }
            return;
        }
        const scale = e.shiftKey ? FINE_SCALE : 1;
        const change: EqSectionChange = {};
        if (drag.moveFreq) {
            change.freqHz = xToFreq(
                geometry,
                drag.nodeX + (x - drag.downX) * scale,
            );
        }
        if (drag.moveGain) {
            change.gainDb = yToGain(
                geometry,
                drag.nodeY + (y - drag.downY) * scale,
            );
        }
        onChange(drag.index, change, 'move', drag.codeOnly);
    };

    const endDrag = (e: React.PointerEvent<HTMLCanvasElement>) => {
        const drag = dragRef.current;
        if (!drag || drag.pointerId !== e.pointerId) {
            return;
        }
        dragRef.current = null;
        onChange(drag.index, {}, 'commit', drag.codeOnly);
    };

    /**
     * Right-click on a node resets its gain to 0 dB; Ctrl/Cmd right-click
     * writes the audio's values over all of the section's code-only edits.
     */
    const handleContextMenu = (e: React.MouseEvent<HTMLCanvasElement>) => {
        e.preventDefault();
        if (!geometry) {
            return;
        }
        const { x, y } = localPoint(e);
        const index = hitTestNode(geometry, sections, x, y);
        if (index === null) {
            return;
        }
        if (isCodeOnly(e)) {
            if (ghosts[index]) {
                onRevert(index, null);
            }
            return;
        }
        const sec = sections[index];
        if (typeHasGain(sec.type) && !sec.driven.gain) {
            onChange(index, { gainDb: 0 }, 'commit', false);
        }
    };

    const handleDoubleClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
        e.stopPropagation();
        if (!geometry) {
            return;
        }
        const { x, y } = localPoint(e);
        const index = hitTestNode(geometry, sections, x, y);
        if (index === null) {
            if (sections.length < MAX_EQ_SECTIONS) {
                onAdd(xToFreq(geometry, x), yToGain(geometry, y));
            }
            return;
        }
        const codeOnly = isCodeOnly(e);
        const base = gestureBase(
            sections[index],
            ghosts[index] ?? null,
            codeOnly,
        );
        if (typeHasGain(base.type) && !base.driven.gain) {
            onChange(index, { gainDb: 0 }, 'commit', codeOnly);
        }
    };

    // Wheel over a node sets its Q. Registered non-passive so the scroll can
    // be kept from the editor; wheels elsewhere scroll the editor as usual.
    const wheelStateRef = useRef({
        editable,
        geometry,
        ghosts,
        onChange,
        sections,
    });
    useLayoutEffect(() => {
        wheelStateRef.current = {
            editable,
            geometry,
            ghosts,
            onChange,
            sections,
        };
    });
    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) {
            return;
        }
        const onWheel = (e: WheelEvent) => {
            const s = wheelStateRef.current;
            if (!s.geometry) {
                return;
            }
            const { x, y } = localPoint(e);
            const index = hitTestNode(s.geometry, s.sections, x, y);
            if (index === null) {
                return;
            }
            e.preventDefault();
            e.stopPropagation();
            const codeOnly = isCodeOnly(e);
            if (codeOnly && !s.editable) {
                return;
            }
            const base = gestureBase(
                s.sections[index],
                s.ghosts[index] ?? null,
                codeOnly,
            );
            if (base.driven.q || base.q === null) {
                return;
            }
            const q = base.q * Math.exp(-e.deltaY * WHEEL_Q_PER_PIXEL);
            s.onChange(index, { q }, 'move', codeOnly);
            if (wheelTimerRef.current !== null) {
                window.clearTimeout(wheelTimerRef.current);
            }
            wheelTimerRef.current = window.setTimeout(() => {
                wheelTimerRef.current = null;
                wheelStateRef.current.onChange(index, {}, 'commit', codeOnly);
            }, WHEEL_COMMIT_MS);
        };
        canvas.addEventListener('wheel', onWheel, { passive: false });
        return () => canvas.removeEventListener('wheel', onWheel);
    }, []);

    useEffect(
        () => () => {
            if (wheelTimerRef.current !== null) {
                window.clearTimeout(wheelTimerRef.current);
            }
        },
        [],
    );

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

    const sel = selected !== null ? sections[selected] : undefined;
    const selGhost = selected !== null ? (ghosts[selected] ?? null) : null;

    const setType = (type: EqSectionType, codeOnly: boolean) => {
        if (selected === null || (codeOnly && !editable)) {
            return;
        }
        onChange(selected, { type }, 'commit', codeOnly);
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
                    onPointerDown={handlePointerDown}
                    onPointerMove={handlePointerMove}
                    onPointerUp={endDrag}
                    onPointerCancel={endDrag}
                    onPointerLeave={() => {
                        setHovered(null);
                        setOverHandle(false);
                    }}
                    onDoubleClick={handleDoubleClick}
                    onContextMenu={handleContextMenu}
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
            <div className="eq-panel-toolbar">
                <div className="eq-channels" role="tablist">
                    {channelLists ? (
                        channelLists.map((_, i) => (
                            <span key={i} className="eq-removable">
                                <button
                                    type="button"
                                    role="tab"
                                    aria-selected={i === channel}
                                    className={
                                        i === channel
                                            ? 'eq-channel eq-channel-active'
                                            : 'eq-channel'
                                    }
                                    title={`Edit channel ${i + 1}'s sections`}
                                    onClick={() => onChannel(i)}
                                >
                                    {channelLabel(i, channelLists.length)}
                                </button>
                                <button
                                    type="button"
                                    className="eq-remove-x"
                                    title={`Remove channel ${i + 1}'s list`}
                                    onClick={() => onRemoveChannel(i)}
                                >
                                    ×
                                </button>
                            </span>
                        ))
                    ) : (
                        <button
                            type="button"
                            role="tab"
                            aria-selected
                            className="eq-channel eq-channel-active"
                            title="One section list for every channel"
                        >
                            all
                        </button>
                    )}
                    <button
                        type="button"
                        className="eq-channel"
                        title={
                            channelLists
                                ? 'Add a channel list, copying this one'
                                : 'Split into per-channel lists (L/R)'
                        }
                        disabled={
                            (channelLists?.length ?? 0) >= MAX_EQ_CHANNEL_LISTS
                        }
                        onClick={onAddChannel}
                    >
                        +
                    </button>
                </div>
                <div className="eq-chips">
                    {sections.map((s, i) => (
                        <span key={i} className="eq-removable">
                            <button
                                type="button"
                                className={
                                    i === selected
                                        ? 'eq-chip eq-chip-selected'
                                        : 'eq-chip'
                                }
                                style={
                                    {
                                        '--chip-color':
                                            SECTION_COLORS[
                                                i % SECTION_COLORS.length
                                            ],
                                    } as React.CSSProperties
                                }
                                title={`Section ${i + 1}: ${TYPE_LABELS[s.type]}`}
                                onClick={() =>
                                    onSelect(i === selected ? null : i)
                                }
                            >
                                {i + 1}
                            </button>
                            <button
                                type="button"
                                className="eq-remove-x"
                                title={`Remove section ${i + 1}`}
                                onClick={() => onRemove(i)}
                            >
                                ×
                            </button>
                        </span>
                    ))}
                    <button
                        type="button"
                        className="eq-chip eq-chip-add"
                        title="Add a section (or double-click the graph)"
                        disabled={sections.length >= MAX_EQ_SECTIONS}
                        onClick={() => onAdd(1000, 0)}
                    >
                        +
                    </button>
                </div>
                {sel ? (
                    <>
                        <div className="eq-types" role="radiogroup">
                            {EQ_SECTION_TYPES.map((t) => {
                                const classes = ['eq-type'];
                                if (sel.type === t) {
                                    classes.push('eq-type-active');
                                }
                                if (
                                    selGhost &&
                                    selGhost.type === t &&
                                    selGhost.type !== sel.type
                                ) {
                                    classes.push('eq-type-ghost');
                                }
                                return (
                                    <button
                                        key={t}
                                        type="button"
                                        role="radio"
                                        aria-checked={sel.type === t}
                                        className={classes.join(' ')}
                                        title={`${TYPE_LABELS[t]} (⌃/⌘-click: edit code only)`}
                                        onClick={(e) =>
                                            setType(t, isCodeOnly(e))
                                        }
                                        onContextMenu={(e) => {
                                            e.preventDefault();
                                            setType(t, true);
                                        }}
                                    >
                                        <svg
                                            width="20"
                                            height="14"
                                            viewBox="0 0 20 14"
                                        >
                                            <path d={TYPE_GLYPHS[t]} />
                                        </svg>
                                    </button>
                                );
                            })}
                        </div>
                        <div className="eq-readouts">
                            <Readout
                                field={FREQ_FIELD}
                                value={sel.freqHz}
                                ghost={selGhost?.freqHz ?? null}
                                driven={sel.driven.freq}
                                editable={editable}
                                onRevert={() => onRevert(selected!, ['freqHz'])}
                                onChange={(v, phase, codeOnly) =>
                                    onChange(
                                        selected!,
                                        v === null ? {} : { freqHz: v },
                                        phase,
                                        codeOnly,
                                    )
                                }
                            />
                            {typeHasGain(sel.type) && (
                                <Readout
                                    field={GAIN_FIELD}
                                    value={sel.gainDb}
                                    ghost={selGhost?.gainDb ?? null}
                                    driven={sel.driven.gain}
                                    editable={editable}
                                    onReset={() =>
                                        onChange(
                                            selected!,
                                            { gainDb: 0 },
                                            'commit',
                                            false,
                                        )
                                    }
                                    onRevert={() =>
                                        onRevert(selected!, ['gainDb'])
                                    }
                                    onChange={(v, phase, codeOnly) =>
                                        onChange(
                                            selected!,
                                            v === null ? {} : { gainDb: v },
                                            phase,
                                            codeOnly,
                                        )
                                    }
                                />
                            )}
                            <Readout
                                field={Q_FIELD}
                                value={sel.q}
                                ghost={selGhost?.q ?? null}
                                driven={sel.driven.q}
                                editable={editable}
                                onReset={() =>
                                    onChange(
                                        selected!,
                                        { q: DEFAULT_Q },
                                        'commit',
                                        false,
                                    )
                                }
                                onRevert={() => onRevert(selected!, ['q'])}
                                onChange={(v, phase, codeOnly) =>
                                    onChange(
                                        selected!,
                                        v === null ? {} : { q: v },
                                        phase,
                                        codeOnly,
                                    )
                                }
                            />
                        </div>
                        <button
                            type="button"
                            className="eq-remove"
                            title="Remove section (Delete)"
                            onClick={() => onRemove(selected!)}
                        >
                            ×
                        </button>
                    </>
                ) : (
                    <div className="eq-hint">
                        {sections.length === 0
                            ? 'double-click the graph or press + to add a section'
                            : 'drag a node · select it, then drag its handles or scroll for Q · double-click to add · right-click resets · ⌃/⌘ edits code only, ⌃/⌘ right-click reverts it'}
                    </div>
                )}
            </div>
        </div>
    );
}
