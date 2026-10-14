/**
 * The graph's pointer gestures: drag a node (freq and gain), drag the
 * selected section's Q handles, scroll over a node for Q, double-click to
 * add a section or reset a node's gain, and right-click to reset or revert.
 */
import React, { useEffect, useLayoutEffect, useRef } from 'react';

import { MAX_EQ_SECTIONS, typeHasGain } from './eqMath';
import type { EqSectionChange, EqSectionView } from './eqPanelModel';
import {
    type EqGeometry,
    hitTestNode,
    hitTestQHandle,
    nodePosition,
    octavesToQ,
    xToFreq,
    yToGain,
} from './eqGeometry';
import { FINE_SCALE, isCodeOnly } from './eqGestures';
import type { EqPanelProps } from './EqPanel';

/** Scroll-to-Q sensitivity: Q multiplies by e^(−deltaY·k). */
const WHEEL_Q_PER_PIXEL = 0.0025;
/** Idle time after the last wheel step before the edit is committed. */
const WHEEL_COMMIT_MS = 300;

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

/** The values a gesture starts from: the ghost for code-only gestures. */
function gestureBase(
    live: EqSectionView,
    ghost: EqSectionView | null,
    codeOnly: boolean,
): EqSectionView {
    return codeOnly && ghost ? ghost : live;
}

interface GraphGestureArgs extends Pick<
    EqPanelProps,
    | 'sections'
    | 'ghosts'
    | 'selected'
    | 'editable'
    | 'onSelect'
    | 'onChange'
    | 'onAdd'
    | 'onRevert'
> {
    canvasRef: React.RefObject<HTMLCanvasElement | null>;
    rootRef: React.RefObject<HTMLDivElement | null>;
    geometry: EqGeometry | null;
    hovered: number | null;
    setHovered: (index: number | null) => void;
    setOverHandle: (over: boolean) => void;
}

export function useEqGraphGestures({
    canvasRef,
    rootRef,
    geometry,
    sections,
    ghosts,
    selected,
    editable,
    hovered,
    setHovered,
    setOverHandle,
    onSelect,
    onChange,
    onAdd,
    onRevert,
}: GraphGestureArgs) {
    const dragRef = useRef<DragState | null>(null);
    const wheelTimerRef = useRef<number | null>(null);

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
        // A press in progress means this is macOS Ctrl+click starting a
        // code-only drag, not a right-click.
        if (!geometry || dragRef.current) {
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
            const rect = canvas.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const y = e.clientY - rect.top;
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
    }, [canvasRef]);

    useEffect(
        () => () => {
            if (wheelTimerRef.current !== null) {
                window.clearTimeout(wheelTimerRef.current);
            }
        },
        [],
    );

    return {
        onContextMenu: handleContextMenu,
        onDoubleClick: handleDoubleClick,
        onPointerCancel: endDrag,
        onPointerDown: handlePointerDown,
        onPointerMove: handlePointerMove,
        onPointerUp: endDrag,
    };
}
