import { useEffect, useRef, useState } from 'react';
import electronAPI from '../electronAPI';
import { VideoRenderer } from '../video/VideoRenderer';

/** The audience-facing window; currently shows the patch's `$v.out`. */
export function PerformanceWindow() {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (canvas === null) return;

        const abort = new AbortController();
        let renderer: VideoRenderer | null = null;
        let unsubscribe: (() => void) | null = null;

        const show = (shader: Parameters<VideoRenderer['setShader']>[0]) => {
            renderer?.setShader(shader).then(
                () => setError(null),
                (e: unknown) =>
                    setError(e instanceof Error ? e.message : String(e)),
            );
        };

        VideoRenderer.create(canvas, abort.signal).then(
            (created) => {
                renderer = created;
                created.setCvSink((values) => {
                    void electronAPI.video.sendCvValues(values);
                });
                created.setPreviewSink((frame) => {
                    void electronAPI.video.sendPreviewFrame(frame);
                });
                const stopShader = electronAPI.video.onShader(show);
                const stopUniform = electronAPI.video.onUniform((updates) => {
                    for (const { slot, value } of updates) {
                        created.setUniform(slot, value);
                    }
                });
                unsubscribe = () => {
                    stopShader();
                    stopUniform();
                };
                void electronAPI.video.getShader().then(show);
            },
            (e: unknown) => {
                if (!abort.signal.aborted) {
                    setError(e instanceof Error ? e.message : String(e));
                }
            },
        );

        return () => {
            abort.abort();
            unsubscribe?.();
            renderer?.dispose();
        };
    }, []);

    return (
        <div style={{ background: '#000', height: '100vh', width: '100vw' }}>
            <canvas
                ref={canvasRef}
                style={{ display: 'block', height: '100%', width: '100%' }}
            />
            {error !== null && (
                <pre
                    style={{
                        color: '#ff6b6b',
                        left: 12,
                        margin: 0,
                        position: 'fixed',
                        top: 12,
                        whiteSpace: 'pre-wrap',
                    }}
                >
                    {error}
                </pre>
            )}
        </div>
    );
}
