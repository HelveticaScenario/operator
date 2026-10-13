import React, { useRef, useState } from 'react';

import { formatGain, formatHz } from './eqMath';
import { type EqChangePhase, FINE_SCALE, isCodeOnly } from './eqGestures';

/** Readout scrub sensitivities; right and up raise the value. */
const SCRUB_FREQ_PER_PIXEL = 0.005;
const SCRUB_GAIN_DB_PER_PIXEL = 0.1;
const SCRUB_Q_PER_PIXEL = 0.01;

/** Marks a readout whose value follows a modulating signal. */
const DRIVEN_MARK = (
    <span className="eq-driven" title="Modulated: follows its signal">
        {' ∿'}
    </span>
);

/** How one readout shows, scrubs, and parses its value. */
export interface ReadoutField {
    title: string;
    format: (v: number) => string;
    /** The value after a scrub of `pixels` from `start`. */
    scrub: (start: number, pixels: number) => number;
    /** Text the type-in box starts with. */
    editText: (v: number) => string;
    /** A typed value, or null when unparsable. */
    parse: (text: string) => number | null;
}

export const FREQ_FIELD: ReadoutField = {
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

export const GAIN_FIELD: ReadoutField = {
    editText: (v) => v.toFixed(1),
    format: formatGain,
    parse: (text) => {
        const v = parseFloat(text.replace(/db/i, ''));
        return Number.isFinite(v) ? v : null;
    },
    scrub: (start, pixels) => start + pixels * SCRUB_GAIN_DB_PER_PIXEL,
    title: 'Gain',
};

export const Q_FIELD: ReadoutField = {
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
export function Readout({
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

    const endScrub = (e: React.PointerEvent) => {
        const s = scrubRef.current;
        if (!s || s.pointerId !== e.pointerId) {
            return;
        }
        scrubRef.current = null;
        onChange(null, 'commit', s.codeOnly);
    };

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
                if (scrubRef.current) {
                    return;
                }
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
            onPointerUp={endScrub}
            onPointerCancel={endScrub}
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
