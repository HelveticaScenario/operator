import React, { useCallback, useEffect, useRef } from 'react';
import type { VuMeterDef, VuMeterGhost } from '../../shared/dsl/vuMeterTypes';
import { METER_PAD_CSS, meterNormToDb } from '../app/vuMeter';
import './VuMeterPanel.css';

/**
 * A Ctrl/Cmd gesture edits the DSL source only, leaving the running audio
 * untouched until the next patch update; the control then shows a ghost of
 * the code-side value. On macOS Ctrl+click arrives as a contextmenu event,
 * so the buttons detect the code-only gesture on that path too.
 */
function isCodeOnly(e: React.MouseEvent | React.PointerEvent): boolean {
    return e.ctrlKey || e.metaKey;
}

const MS_BUTTON_HINT = '⌃/⌘-click: edit code only';

export const VU_PANEL_MIN_HEIGHT = 84;
export const VU_PANEL_MAX_HEIGHT = 480;
/** Strip height at which the pan knob appears. */
const PAN_MIN_HEIGHT = 150;
/** Strip height at which the peak-readout pill appears. */
const READOUT_MIN_HEIGHT = 180;
/** Pan change per vertical drag pixel (full ±5 range over 100px). */
const PAN_PER_PIXEL = 0.1;

interface VuMeterPanelProps {
    outputs: VuMeterDef[];
    /** Panel height in px; the drag handle on the top edge changes it. */
    height: number;
    onHeightChange: (height: number) => void;
    /** Called once at drag end with the final height, for persistence. */
    onHeightCommit: (height: number) => void;
    onToggleMute: (key: string, codeOnly: boolean) => void;
    onToggleSolo: (key: string, codeOnly: boolean) => void;
    /** Code-side values diverging from the audio, keyed by meter key. */
    ghosts: Map<string, VuMeterGhost>;
    /** Live pan while dragging. `codeOnly` gestures edit the source only. */
    onPanChange: (key: string, pan: number, codeOnly: boolean) => void;
    /** Drag released — flush the final pan into the source once. */
    onPanCommit: (key: string, pan: number, codeOnly: boolean) => void;
    /** Live fader drag on the meter: absolute position mapped to dB. */
    onGainChange: (key: string, db: number, codeOnly: boolean) => void;
    /** Drag released — flush the final gain into the source once. */
    onGainCommit: (key: string, codeOnly: boolean) => void;
    /** Right-click on the meter: reset gain to the default. */
    onGainReset: (key: string) => void;
    /** Ctrl/Cmd right-click: revert the source gain to the audio's. */
    onGainRevert: (key: string) => void;
    onPeakReset: (key: string) => void;
    /** The canvas backing store changed size and needs an immediate redraw. */
    onCanvasResized: (key: string, canvas: HTMLCanvasElement) => void;
    registerCanvas: (key: string, canvas: HTMLCanvasElement) => void;
    unregisterCanvas: (key: string) => void;
    registerReadout: (key: string, el: HTMLElement) => void;
    unregisterReadout: (key: string) => void;
    /** Locked pan knobs register their pointer line for live rotation. */
    registerPanPointer: (key: string, el: SVGLineElement) => void;
    unregisterPanPointer: (key: string) => void;
}

/**
 * Pan knob: a pointer line inside a circle with a fixed center-detent
 * triangle above it. Vertical drag changes the pan; a Ctrl/Cmd drag edits
 * the source only, showing a faded ghost pointer at the code-side value
 * while it diverges from the audio. Right-click recenters; Ctrl/Cmd
 * right-click reverts the source to the audio's value, dropping the ghost.
 * A locked knob (signal-driven pan) takes no input; its pointer line is
 * registered so the RAF loop can rotate it live with the signal.
 */
function PanKnob({
    pan,
    ghostPan,
    locked,
    onPanChange,
    onPanCommit,
    registerPointer,
}: {
    pan: number;
    /** Code-side pan while it diverges from the audio; undefined hides it. */
    ghostPan: number | undefined;
    locked: boolean;
    onPanChange: (pan: number, codeOnly: boolean) => void;
    onPanCommit: (pan: number, codeOnly: boolean) => void;
    registerPointer: (el: SVGLineElement | null) => void;
}) {
    const dragRef = useRef<{
        startY: number;
        startPan: number;
        lastPan: number;
        codeOnly: boolean;
    } | null>(null);

    const handlePointerDown = useCallback(
        (e: React.PointerEvent<HTMLDivElement>) => {
            // Right presses belong to the contextmenu (revert) gesture.
            if (e.button !== 0) {
                return;
            }
            e.preventDefault();
            e.currentTarget.setPointerCapture(e.pointerId);
            const codeOnly = isCodeOnly(e);
            // A code-only drag continues from the code-side value, not the
            // audio's.
            const startPan = codeOnly ? (ghostPan ?? pan) : pan;
            dragRef.current = {
                codeOnly,
                lastPan: startPan,
                startPan,
                startY: e.clientY,
            };
        },
        [pan, ghostPan],
    );

    const handlePointerMove = useCallback(
        (e: React.PointerEvent<HTMLDivElement>) => {
            const drag = dragRef.current;
            if (!drag) {
                return;
            }
            const raw =
                drag.startPan + (drag.startY - e.clientY) * PAN_PER_PIXEL;
            const next = Math.round(Math.min(5, Math.max(-5, raw)) * 10) / 10;
            if (next !== drag.lastPan) {
                drag.lastPan = next;
                onPanChange(next, drag.codeOnly);
            }
        },
        [onPanChange],
    );

    // Also bound to pointercancel/lostpointercapture: a drag that loses its
    // capture must end here, or button-less hover moves keep changing the
    // pan and the final value never commits. Re-entry after a normal
    // pointerup is a no-op (dragRef is already null).
    const handlePointerUp = useCallback(() => {
        const drag = dragRef.current;
        dragRef.current = null;
        if (drag && drag.lastPan !== drag.startPan) {
            onPanCommit(drag.lastPan, drag.codeOnly);
        }
    }, [onPanCommit]);

    // Pan −5…+5 sweeps the pointer line ±135° from vertical.
    const angle = (pan / 5) * 135;

    return (
        <div
            className={`vu-pan-knob${locked ? ' vu-pan-knob--locked' : ''}`}
            title={
                locked
                    ? 'Pan (signal-controlled)'
                    : `Pan ${pan} (drag; ⌃/⌘ drag: edit code only; right-click: center; ⌃/⌘ right-click: revert)`
            }
            onPointerDown={locked ? undefined : handlePointerDown}
            onPointerMove={locked ? undefined : handlePointerMove}
            onPointerUp={locked ? undefined : handlePointerUp}
            onPointerCancel={locked ? undefined : handlePointerUp}
            onLostPointerCapture={locked ? undefined : handlePointerUp}
            onContextMenu={
                locked
                    ? undefined
                    : (e) => {
                          e.preventDefault();
                          if (isCodeOnly(e)) {
                              // Revert the source to the audio's pan; no-op
                              // when they already agree.
                              if (ghostPan !== undefined) {
                                  onPanChange(pan, true);
                                  onPanCommit(pan, true);
                              }
                              return;
                          }
                          onPanChange(0, false);
                          onPanCommit(0, false);
                      }
            }
        >
            <svg viewBox="0 0 26 30">
                <path d="M10 1 L16 1 L13 5 Z" fill="currentColor" />
                <circle
                    cx="13"
                    cy="17"
                    r="9"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.6"
                />
                {ghostPan !== undefined && (
                    <line
                        className="vu-pan-ghost"
                        x1="13"
                        y1="17"
                        x2="13"
                        y2="8"
                        stroke="currentColor"
                        strokeWidth="1.6"
                        transform={`rotate(${(ghostPan / 5) * 135} 13 17)`}
                    />
                )}
                <line
                    ref={locked ? registerPointer : undefined}
                    x1="13"
                    y1="17"
                    x2="13"
                    y2="8"
                    stroke="currentColor"
                    strokeWidth="1.6"
                    transform={`rotate(${angle} 13 17)`}
                />
            </svg>
        </div>
    );
}

interface VuMeterProps {
    output: VuMeterDef;
    /** Code-side values diverging from the audio for this meter. */
    ghost: VuMeterGhost | undefined;
    /** 1-based position, shown on the activator button like a track number. */
    index: number;
    /** True when mute/solo state leaves this output inaudible. */
    suppressed: boolean;
    showReadout: boolean;
    showPan: boolean;
    onToggleMute: (key: string, codeOnly: boolean) => void;
    onToggleSolo: (key: string, codeOnly: boolean) => void;
    onPanChange: (key: string, pan: number, codeOnly: boolean) => void;
    onPanCommit: (key: string, pan: number, codeOnly: boolean) => void;
    onGainChange: (key: string, db: number, codeOnly: boolean) => void;
    onGainCommit: (key: string, codeOnly: boolean) => void;
    onGainReset: (key: string) => void;
    onGainRevert: (key: string) => void;
    onPeakReset: (key: string) => void;
    onCanvasResized: (key: string, canvas: HTMLCanvasElement) => void;
    registerCanvas: (key: string, canvas: HTMLCanvasElement) => void;
    unregisterCanvas: (key: string) => void;
    registerReadout: (key: string, el: HTMLElement) => void;
    unregisterReadout: (key: string) => void;
    registerPanPointer: (key: string, el: SVGLineElement) => void;
    unregisterPanPointer: (key: string) => void;
}

function VuMeter({
    output,
    ghost,
    index,
    suppressed,
    showReadout,
    showPan,
    onToggleMute,
    onToggleSolo,
    onPanChange,
    onPanCommit,
    onGainChange,
    onGainCommit,
    onGainReset,
    onGainRevert,
    onPeakReset,
    onCanvasResized,
    registerCanvas,
    unregisterCanvas,
    registerReadout,
    unregisterReadout,
    registerPanPointer,
    unregisterPanPointer,
}: VuMeterProps) {
    const canvasRef = useRef<HTMLCanvasElement | null>(null);
    /** Non-null while a fader drag is live; the modifier state is sampled
     *  once at pointerdown and holds for the whole drag. */
    const faderDragRef = useRef<{ codeOnly: boolean } | null>(null);

    const panPointerRef = useCallback(
        (el: SVGLineElement | null) => {
            if (el) {
                registerPanPointer(output.key, el);
            } else {
                unregisterPanPointer(output.key);
            }
        },
        [output.key, registerPanPointer, unregisterPanPointer],
    );

    // Size the backing store to device pixels and keep it matched while the
    // panel is resized. Reassigning canvas.width clears the canvas, so only
    // touch it on a real change, and ask for an immediate redraw after —
    // the RAF loop only repaints while the clock runs. Registration comes
    // first so the mount-time redraw finds the canvas and paints the panel
    // as soon as it opens.
    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) {
            return;
        }
        canvas.dataset.tapModuleId = output.moduleId;
        canvas.dataset.channels = String(output.channels);
        registerCanvas(output.key, canvas);
        const resize = () => {
            const dpr = window.devicePixelRatio || 1;
            const rect = canvas.getBoundingClientRect();
            const width = Math.max(1, Math.round(rect.width * dpr));
            const height = Math.max(1, Math.round(rect.height * dpr));
            if (canvas.width === width && canvas.height === height) {
                return;
            }
            canvas.width = width;
            canvas.height = height;
            onCanvasResized(output.key, canvas);
        };
        resize();
        // First paint regardless of whether the default backing-store size
        // happened to match.
        onCanvasResized(output.key, canvas);
        const observer = new ResizeObserver(resize);
        observer.observe(canvas);
        return () => {
            observer.disconnect();
            unregisterCanvas(output.key);
        };
    }, [
        output.key,
        output.moduleId,
        output.channels,
        onCanvasResized,
        registerCanvas,
        unregisterCanvas,
    ]);

    /** Map a pointer event on the canvas to a dB position on the scale. */
    const eventToDb = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const norm =
            (e.clientY - rect.top - METER_PAD_CSS) /
            Math.max(1, rect.height - METER_PAD_CSS * 2);
        return meterNormToDb(norm);
    }, []);

    const faderEnabled = output.gainModuleId !== null;

    const handleFaderDown = useCallback(
        (e: React.PointerEvent<HTMLCanvasElement>) => {
            // Right presses belong to the contextmenu (reset/revert) gesture.
            if (!faderEnabled || e.button !== 0) {
                return;
            }
            e.preventDefault();
            e.currentTarget.setPointerCapture(e.pointerId);
            const codeOnly = isCodeOnly(e);
            faderDragRef.current = { codeOnly };
            onGainChange(output.key, eventToDb(e), codeOnly);
        },
        [faderEnabled, output.key, onGainChange, eventToDb],
    );

    const handleFaderMove = useCallback(
        (e: React.PointerEvent<HTMLCanvasElement>) => {
            const drag = faderDragRef.current;
            if (drag) {
                onGainChange(output.key, eventToDb(e), drag.codeOnly);
            }
        },
        [output.key, onGainChange, eventToDb],
    );

    // Also bound to pointercancel/lostpointercapture: a drag that loses its
    // capture must end here, or button-less hover moves keep rewriting the
    // gain and the final value never flushes to the source. Re-entry after
    // a normal pointerup is a no-op (the drag ref is already null).
    const handleFaderUp = useCallback(() => {
        const drag = faderDragRef.current;
        if (drag) {
            faderDragRef.current = null;
            onGainCommit(output.key, drag.codeOnly);
        }
    }, [output.key, onGainCommit]);

    const readoutRef = useCallback(
        (el: HTMLButtonElement | null) => {
            if (el) {
                registerReadout(output.key, el);
            } else {
                unregisterReadout(output.key);
            }
        },
        [output.key, registerReadout, unregisterReadout],
    );

    const channelBadge =
        output.channels === 2
            ? `ch ${output.baseChannel}–${output.baseChannel + 1}`
            : `ch ${output.baseChannel}`;

    return (
        <div
            className={`vu-meter${suppressed ? ' vu-meter--suppressed' : ''}${
                output.main ? ' vu-meter--main' : ''
            }`}
            data-vu-key={output.key}
            title={output.main ? 'End of chain' : `${output.key} (${channelBadge})`}
        >
            <div className="vu-meter-header">
                <span className="vu-meter-label">
                    {output.label ?? output.key}
                </span>
            </div>
            <div className="vu-meter-body">
                <div className="vu-meter-controls">
                    {showReadout && (
                        <button
                            ref={readoutRef}
                            className="vu-peak-readout"
                            title="Peak level (click to reset)"
                            onClick={() => onPeakReset(output.key)}
                        >
                            -∞
                        </button>
                    )}
                    <div className="vu-meter-controls-spacer" />
                    {showPan &&
                        (output.panModuleId != null || output.panLocked) && (
                            <PanKnob
                                pan={output.pan ?? 0}
                                ghostPan={ghost?.pan}
                                locked={output.panLocked}
                                onPanChange={(pan, codeOnly) =>
                                    onPanChange(output.key, pan, codeOnly)
                                }
                                onPanCommit={(pan, codeOnly) =>
                                    onPanCommit(output.key, pan, codeOnly)
                                }
                                registerPointer={panPointerRef}
                            />
                        )}
                    {output.muteModuleId != null && (
                        <>
                            {/* Outer ring = code state, inner core = audio
                                state; they only differ while a code-only
                                edit awaits a patch update. */}
                            <button
                                className="vu-btn vu-btn-mute"
                                aria-pressed={output.mute}
                                data-code-pressed={ghost?.mute ?? output.mute}
                                title={`${output.mute ? 'Unmute' : 'Mute'} (${MS_BUTTON_HINT})`}
                                onClick={(e) =>
                                    onToggleMute(output.key, isCodeOnly(e))
                                }
                                onContextMenu={(e) => {
                                    if (e.ctrlKey) {
                                        e.preventDefault();
                                        e.stopPropagation();
                                        onToggleMute(output.key, true);
                                    }
                                }}
                            >
                                <span className="vu-btn-inner">{index}</span>
                            </button>
                            <button
                                className="vu-btn vu-btn-solo"
                                aria-pressed={output.solo}
                                data-code-pressed={ghost?.solo ?? output.solo}
                                title={`Solo (${MS_BUTTON_HINT})`}
                                onClick={(e) =>
                                    onToggleSolo(output.key, isCodeOnly(e))
                                }
                                onContextMenu={(e) => {
                                    if (e.ctrlKey) {
                                        e.preventDefault();
                                        e.stopPropagation();
                                        onToggleSolo(output.key, true);
                                    }
                                }}
                            >
                                <span className="vu-btn-inner">S</span>
                            </button>
                        </>
                    )}
                </div>
                <div className="vu-meter-canvas-wrap">
                    <canvas
                        ref={canvasRef}
                        className={`vu-meter-canvas${
                            faderEnabled ? ' vu-meter-canvas--fader' : ''
                        }${output.gainLocked ? ' vu-meter-canvas--locked' : ''}`}
                        title={
                            output.gainLocked
                                ? 'Gain (signal-controlled)'
                                : faderEnabled
                                  ? 'Gain (drag; ⌃/⌘ drag: edit code only; right-click: reset; ⌃/⌘ right-click: revert)'
                                  : undefined
                        }
                        onPointerDown={handleFaderDown}
                        onPointerMove={handleFaderMove}
                        onPointerUp={handleFaderUp}
                        onPointerCancel={handleFaderUp}
                        onLostPointerCapture={handleFaderUp}
                        onContextMenu={
                            faderEnabled
                                ? (e) => {
                                      e.preventDefault();
                                      if (isCodeOnly(e)) {
                                          onGainRevert(output.key);
                                      } else {
                                          onGainReset(output.key);
                                      }
                                  }
                                : undefined
                        }
                    />
                </div>
            </div>
        </div>
    );
}

export function VuMeterPanel({
    outputs,
    height,
    onHeightChange,
    onHeightCommit,
    onToggleMute,
    onToggleSolo,
    ghosts,
    onPanChange,
    onPanCommit,
    onGainChange,
    onGainCommit,
    onGainReset,
    onGainRevert,
    onPeakReset,
    onCanvasResized,
    registerCanvas,
    unregisterCanvas,
    registerReadout,
    unregisterReadout,
    registerPanPointer,
    unregisterPanPointer,
}: VuMeterPanelProps) {
    const channelOutputs = outputs.filter((o) => !o.main);
    const mainOutput = outputs.find((o) => o.main) ?? null;
    const anySolo = channelOutputs.some((o) => o.solo);
    const dragState = useRef<{ startY: number; startHeight: number } | null>(
        null,
    );
    const heightRef = useRef(height);
    useEffect(() => {
        heightRef.current = height;
    }, [height]);

    const handleDragStart = useCallback(
        (e: React.PointerEvent<HTMLDivElement>) => {
            e.preventDefault();
            dragState.current = {
                startHeight: heightRef.current,
                startY: e.clientY,
            };
            const onMove = (move: PointerEvent) => {
                const drag = dragState.current;
                if (!drag) {
                    return;
                }
                // The panel is docked at the bottom, so dragging up grows it.
                const next = Math.min(
                    VU_PANEL_MAX_HEIGHT,
                    Math.max(
                        VU_PANEL_MIN_HEIGHT,
                        drag.startHeight + (drag.startY - move.clientY),
                    ),
                );
                onHeightChange(next);
            };
            const onUp = () => {
                dragState.current = null;
                window.removeEventListener('pointermove', onMove);
                window.removeEventListener('pointerup', onUp);
                window.removeEventListener('pointercancel', onUp);
                onHeightCommit(heightRef.current);
            };
            window.addEventListener('pointermove', onMove);
            window.addEventListener('pointerup', onUp);
            window.addEventListener('pointercancel', onUp);
        },
        [onHeightChange, onHeightCommit],
    );

    const meterProps = {
        onCanvasResized,
        onGainChange,
        onGainCommit,
        onGainReset,
        onGainRevert,
        onPanChange,
        onPanCommit,
        onPeakReset,
        onToggleMute,
        onToggleSolo,
        registerCanvas,
        registerPanPointer,
        registerReadout,
        showPan: height >= PAN_MIN_HEIGHT,
        showReadout: height >= READOUT_MIN_HEIGHT,
        unregisterCanvas,
        unregisterPanPointer,
        unregisterReadout,
    };

    return (
        <div className="vu-meter-panel" style={{ height }}>
            <div
                className="vu-panel-resize-handle"
                title="Drag to resize"
                onPointerDown={handleDragStart}
            />
            <div className="vu-meter-scroll">
                {channelOutputs.length === 0 ? (
                    <div className="vu-meter-empty">
                        no outputs — add .out() to the patch
                    </div>
                ) : (
                    channelOutputs.map((output, i) => (
                        <VuMeter
                            key={output.key}
                            output={output}
                            ghost={ghosts.get(output.key)}
                            index={i + 1}
                            suppressed={
                                anySolo ? !output.solo : output.mute
                            }
                            {...meterProps}
                        />
                    ))
                )}
            </div>
            {mainOutput && (
                <VuMeter
                    key={mainOutput.key}
                    output={mainOutput}
                    ghost={ghosts.get(mainOutput.key)}
                    index={0}
                    suppressed={false}
                    {...meterProps}
                />
            )}
        </div>
    );
}
