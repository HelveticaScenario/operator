import { useEffect, useRef, useState } from 'react';
import { performanceOutput } from '../../video/PerformanceOutput';

/**
 * A small copy of the performance window in the corner of the editor, shown
 * while that window is open, so the performer sees exactly what the audience
 * sees: the picture and the code as laid over it.
 */
export function PerformancePreview() {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const [size, setSize] = useState(performanceOutput.size);

    useEffect(
        () =>
            performanceOutput.subscribe(() => {
                setSize(performanceOutput.size);
            }),
        [],
    );

    const open = size !== null;
    useEffect(() => {
        if (!open) return;
        return performanceOutput.onComposite((source) => {
            const canvas = canvasRef.current;
            const context = canvas?.getContext('2d');
            if (!canvas || !context) return;
            const scale = window.devicePixelRatio;
            const width = Math.max(1, Math.round(canvas.clientWidth * scale));
            const height = Math.max(1, Math.round(canvas.clientHeight * scale));
            if (canvas.width !== width || canvas.height !== height) {
                canvas.width = width;
                canvas.height = height;
            }
            context.imageSmoothingQuality = 'high';
            context.drawImage(source, 0, 0, width, height);
        });
    }, [open]);

    if (size === null) return null;
    return (
        <canvas
            ref={canvasRef}
            className="performance-preview"
            style={{ aspectRatio: `${size.width} / ${size.height}` }}
        />
    );
}
