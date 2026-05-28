import { useEffect, useRef } from 'react';
import electronAPI from '../../electronAPI';
import {
    createScopeXY,
    type ScopeXYPairData,
    type ScopeXY,
} from './pipeline';

type Rgb = [number, number, number];

const DEFAULT_BEAM: Rgb = [0.05, 1.0, 0.35];
const DEFAULT_BG: Rgb = [0.005, 0.008, 0.012];

function parseCssColor(value: string, fallback: Rgb): Rgb {
    const trimmed = value.trim();
    if (!trimmed) return fallback;
    if (trimmed.startsWith('#')) {
        const hex = trimmed.slice(1);
        let r: number;
        let g: number;
        let b: number;
        if (hex.length === 3) {
            r = parseInt(hex[0] + hex[0], 16);
            g = parseInt(hex[1] + hex[1], 16);
            b = parseInt(hex[2] + hex[2], 16);
        } else if (hex.length === 6 || hex.length === 8) {
            r = parseInt(hex.slice(0, 2), 16);
            g = parseInt(hex.slice(2, 4), 16);
            b = parseInt(hex.slice(4, 6), 16);
        } else {
            return fallback;
        }
        if (Number.isNaN(r) || Number.isNaN(g) || Number.isNaN(b)) {
            return fallback;
        }
        return [r / 255, g / 255, b / 255];
    }
    const match = trimmed.match(
        /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i,
    );
    if (match) {
        return [
            parseInt(match[1], 10) / 255,
            parseInt(match[2], 10) / 255,
            parseInt(match[3], 10) / 255,
        ];
    }
    return fallback;
}

function readScopeColorsRgb(): { beam: Rgb; background: Rgb } {
    const styles = getComputedStyle(document.documentElement);
    return {
        beam: parseCssColor(
            styles.getPropertyValue('--accent-primary'),
            DEFAULT_BEAM,
        ),
        background: parseCssColor(
            styles.getPropertyValue('--bg-primary'),
            DEFAULT_BG,
        ),
    };
}

interface ScopeXYBackgroundProps {
    /**
     * When true, the RAF loop pauses and the canvas retains whatever was last
     * drawn. Used to keep one canvas per editor buffer alive while only the
     * active buffer's canvas updates.
     */
    paused?: boolean;
    /** Beam intensity multiplier (0..1). */
    intensity?: number;
    /**
     * Phosphor persistence (0..1). 1 = no fade (infinite trail), 0 = full
     * clear each frame. Converted internally to `fadeAmount = 1 - persistence`.
     */
    persistence?: number;
    /** Toggle GPU Lanczos upscaling. */
    upsample?: boolean;
}

const DEFAULT_INTENSITY = 0.6;
// 0.6 ≈ fadeAmount 0.4 — matches dood.al's default per-frame phosphor decay.
// Higher values keep persistence trails visible much longer than the
// reference; lower (toward 0) clears each frame.
const DEFAULT_PERSISTENCE = 0.6;
const DEFAULT_UPSAMPLE = true;

/**
 * Full-bleed Lissajous oscilloscope canvas that lives behind the editor.
 * Polls `synthesizer.getScopeXy()` on every RAF tick and redraws. The
 * underlying renderer is a port of m1el/woscope + dood.al/oscilloscope —
 * see pipeline.ts for the attribution / pass-by-pass breakdown.
 */
export function ScopeXYBackground({
    paused = false,
    intensity = DEFAULT_INTENSITY,
    persistence = DEFAULT_PERSISTENCE,
    upsample = DEFAULT_UPSAMPLE,
}: ScopeXYBackgroundProps = {}) {
    const canvasRef = useRef<HTMLCanvasElement | null>(null);
    const pipelineRef = useRef<ScopeXY | null>(null);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.max(1, Math.floor(canvas.clientWidth * dpr));
        canvas.height = Math.max(1, Math.floor(canvas.clientHeight * dpr));

        const initialColors = readScopeColorsRgb();
        let pipeline: ScopeXY;
        try {
            pipeline = createScopeXY(canvas, {
                color: initialColors.beam,
                background: initialColors.background,
                intensity,
                fadeAmount: 1 - persistence,
                upsample,
            });
        } catch (err) {
            console.warn('xy scope: failed to initialise WebGL', err);
            return;
        }
        pipelineRef.current = pipeline;

        const ro = new ResizeObserver(() => {
            const d = window.devicePixelRatio || 1;
            const w = Math.max(1, Math.floor(canvas.clientWidth * d));
            const h = Math.max(1, Math.floor(canvas.clientHeight * d));
            pipeline.resize(w, h);
        });
        ro.observe(canvas);

        return () => {
            ro.disconnect();
            pipeline.dispose();
            pipelineRef.current = null;
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps -- initial intensity/persistence applied on mount; runtime changes are picked up via the setIntensity / setFadeAmount effects below.
    }, []);

    useEffect(() => {
        pipelineRef.current?.setIntensity(intensity);
    }, [intensity]);

    useEffect(() => {
        pipelineRef.current?.setFadeAmount(1 - persistence);
    }, [persistence]);

    useEffect(() => {
        pipelineRef.current?.setUpsample(upsample);
    }, [upsample]);

    useEffect(() => {
        if (paused) return;
        let cancelled = false;
        let rafId = 0;
        // Audio thread holds the scope_xy mutex briefly each callback. When
        // the renderer polls during that window try_lock fails and the IPC
        // returns an empty Vec — drawing that as []-data clears the canvas
        // to background and produces a single-frame flicker. Skip transient
        // empties; only clear after a few consecutive empty frames (real
        // engine stop or patch removal of $scopeXY).
        const EMPTY_FRAMES_BEFORE_CLEAR = 5;
        let consecutiveEmpty = 0;
        const tick = () => {
            if (cancelled) return;
            const pipeline = pipelineRef.current;
            if (!pipeline) {
                rafId = requestAnimationFrame(tick);
                return;
            }
            electronAPI.synthesizer
                .getScopeXy()
                .then((rows) => {
                    if (cancelled) return;
                    if (rows.length === 0) {
                        consecutiveEmpty++;
                        if (consecutiveEmpty >= EMPTY_FRAMES_BEFORE_CLEAR) {
                            const { beam, background } =
                                readScopeColorsRgb();
                            pipeline.setColors(beam, background);
                            pipeline.draw([]);
                        }
                    } else {
                        consecutiveEmpty = 0;
                        const pairs: ScopeXYPairData[] = rows.map(
                            ([, x, y, head]) => ({ head, x, y }),
                        );
                        const { beam, background } = readScopeColorsRgb();
                        pipeline.setColors(beam, background);
                        pipeline.draw(pairs);
                    }
                    rafId = requestAnimationFrame(tick);
                })
                .catch((err) => {
                    if (cancelled) return;
                    console.error('xy scope: getScopeXy failed', err);
                    rafId = requestAnimationFrame(tick);
                });
        };
        rafId = requestAnimationFrame(tick);
        return () => {
            cancelled = true;
            cancelAnimationFrame(rafId);
        };
    }, [paused]);

    return (
        <canvas
            ref={canvasRef}
            className={
                paused
                    ? 'scope-xy-background scope-xy-background--paused'
                    : 'scope-xy-background'
            }
        />
    );
}
