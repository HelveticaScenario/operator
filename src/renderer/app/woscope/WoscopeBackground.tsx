import { useEffect, useRef } from 'react';
import electronAPI from '../../electronAPI';
import { createWoscope, type ScopeXYPairData, type Woscope } from './pipeline';

/**
 * Full-bleed Lissajous oscilloscope canvas that lives behind the editor.
 * Polls `synthesizer.getScopeXy()` on every RAF tick and redraws.
 */
export function WoscopeBackground() {
    const canvasRef = useRef<HTMLCanvasElement | null>(null);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.max(
            1,
            Math.floor(canvas.clientWidth * dpr),
        );
        canvas.height = Math.max(
            1,
            Math.floor(canvas.clientHeight * dpr),
        );

        let pipeline: Woscope;
        try {
            pipeline = createWoscope(canvas);
        } catch (err) {
            console.warn('woscope: failed to initialise WebGL', err);
            return;
        }

        const ro = new ResizeObserver(() => {
            const d = window.devicePixelRatio || 1;
            const w = Math.max(1, Math.floor(canvas.clientWidth * d));
            const h = Math.max(1, Math.floor(canvas.clientHeight * d));
            pipeline.resize(w, h);
        });
        ro.observe(canvas);

        let cancelled = false;
        let rafId = 0;

        const tick = () => {
            if (cancelled) return;
            electronAPI.synthesizer
                .getScopeXy()
                .then((rows) => {
                    if (cancelled) return;
                    const pairs: ScopeXYPairData[] = rows.map(
                        ([, x, y, head]) => ({
                            head,
                            x,
                            y,
                        }),
                    );
                    pipeline.draw(pairs);
                    rafId = requestAnimationFrame(tick);
                })
                .catch((err) => {
                    if (cancelled) return;
                    console.error('woscope: getScopeXy failed', err);
                    rafId = requestAnimationFrame(tick);
                });
        };
        rafId = requestAnimationFrame(tick);

        return () => {
            cancelled = true;
            cancelAnimationFrame(rafId);
            ro.disconnect();
            pipeline.dispose();
        };
    }, []);

    return <canvas ref={canvasRef} className="woscope-background" />;
}
