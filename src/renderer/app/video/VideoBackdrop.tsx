import { useEffect, useRef, useState } from 'react';
import type { CompiledVideoShader } from '../../../shared/video/videoGraph';
import electronAPI from '../../electronAPI';
import { performanceOutput } from '../../video/PerformanceOutput';
import { VideoRenderer } from '../../video/VideoRenderer';

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * The patch's video, drawn once and shown behind the code. With no
 * performance window the picture fills the whole editor area. With one open,
 * the picture has that window's resolution and aspect ratio and is fitted
 * inside the area, and each frame is copied into that window, so the
 * performer sees exactly what the audience sees. `dim` darkens only the
 * picture behind the code, from 0 (as drawn) to 1 (black), so the code stays
 * readable; the performance window is not dimmed.
 */
export function VideoBackdrop({ dim }: { dim: number }) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const [renderer, setRenderer] = useState<VideoRenderer | null>(null);
    const [shown, setShown] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [open, setOpen] = useState(performanceOutput.isOpen);
    const [size, setSize] = useState(performanceOutput.size);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (canvas === null) return;
        const abort = new AbortController();
        let created: VideoRenderer | null = null;
        let creating = false;
        let latest: CompiledVideoShader | null = null;
        let pushed = false;

        // The renderer is made when the first shader arrives, so a window
        // that never runs video never opens a GPU device.
        const apply = () => {
            if (abort.signal.aborted) return;
            setShown(latest?.hasOutput ?? false);
            if (created !== null) {
                created.setShader(latest).then(
                    () => setError(null),
                    (e: unknown) => setError(errorText(e)),
                );
                return;
            }
            if (latest === null || creating) return;
            creating = true;
            VideoRenderer.create(canvas, abort.signal).then(
                (made) => {
                    creating = false;
                    created = made;
                    made.setPullSource(() => electronAPI.video.pull());
                    made.setErrorSink(setError);
                    made.setCvSink((values) => {
                        void electronAPI.video.sendCvValues(values);
                    });
                    made.setPreviewSink((frame) => {
                        void electronAPI.video.sendPreviewFrame(frame);
                    });
                    setRenderer(made);
                    apply();
                },
                (e: unknown) => {
                    creating = false;
                    if (!abort.signal.aborted) setError(errorText(e));
                },
            );
        };

        const stopShader = electronAPI.video.onShader((shader) => {
            pushed = true;
            latest = shader;
            apply();
        });
        const stopUniform = electronAPI.video.onUniform((updates) => {
            for (const { slot, value } of updates) {
                created?.setUniform(slot, value);
            }
        });
        void electronAPI.video.getShader().then((shader) => {
            if (pushed) return;
            latest = shader;
            apply();
        });
        return () => {
            abort.abort();
            stopShader();
            stopUniform();
            created?.dispose();
        };
    }, []);

    useEffect(
        () =>
            performanceOutput.subscribe(() => {
                setOpen(performanceOutput.isOpen);
                setSize(performanceOutput.size);
            }),
        [],
    );

    useEffect(() => {
        void electronAPI.performanceWindow.setOpen(open);
    }, [open]);

    // The panel gets a dark halo behind its text while a picture shows.
    useEffect(() => {
        const panel = canvasRef.current?.parentElement;
        panel?.classList.toggle('has-video-backdrop', shown);
        return () => panel?.classList.remove('has-video-backdrop');
    }, [shown]);

    useEffect(() => {
        renderer?.setFixedSize(size);
        renderer?.setFrameSink(
            open ? (canvas) => performanceOutput.show(canvas) : null,
        );
    }, [renderer, open, size]);

    return (
        <>
            <canvas
                ref={canvasRef}
                className="video-backdrop"
                style={{
                    filter: `brightness(${1 - dim})`,
                    objectFit: size === null ? 'fill' : 'contain',
                    visibility: shown ? 'visible' : 'hidden',
                }}
            />
            {shown && error !== null && (
                <pre className="video-backdrop-error">{error}</pre>
            )}
        </>
    );
}
