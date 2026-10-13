import type {
    VideoPreviewFrame,
    VideoPreviewView,
} from '../../shared/video/videoGraph';

/** Height of the waveform monitor's brightness axis, in pixels. */
export const WAVEFORM_HEIGHT = 128;
/** Side of the square vectorscope, in pixels. */
export const VECTORSCOPE_SIZE = 144;

/** The brightness, 0..1, of pixel `i` of RGBA8 `data` as it shows over black. */
const luma = (data: Uint8Array, i: number): number =>
    ((0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2]) *
        data[i + 3]) /
    (255 * 255);

/**
 * What shows behind a preview picture, so transparent parts read as such. It
 * stops at the content box, so the canvas's translucent border stays plain.
 */
const CHECKERBOARD =
    'repeating-conic-gradient(#555 0% 25%, #888 0% 50%) 50% / 12px 12px content-box';

/**
 * Writes green phosphor into a black RGBA image. Every plotted sample shows at
 * least a quarter bright, and the busiest one is full, so a sparse trace is as
 * readable as a dense one.
 */
function phosphor(counts: Float32Array): Uint8ClampedArray {
    let max = 0;
    for (const count of counts) max = Math.max(max, count);
    const pixels = new Uint8ClampedArray(counts.length * 4);
    for (let i = 0; i < counts.length; i++) {
        const level =
            counts[i] > 0 ? 0.25 + 0.75 * Math.sqrt(counts[i] / max) : 0;
        pixels[i * 4] = level * 90;
        pixels[i * 4 + 1] = level * 255;
        pixels[i * 4 + 2] = level * 120;
        pixels[i * 4 + 3] = 255;
    }
    return pixels;
}

/**
 * A waveform monitor: each column of the picture plots the brightness of its
 * pixels over black, with white at the top. Returns RGBA pixels of `frame.width` by
 * {@link WAVEFORM_HEIGHT}.
 */
export function waveformPixels(frame: VideoPreviewFrame): Uint8ClampedArray {
    const { data, width, height } = frame;
    const counts = new Float32Array(width * WAVEFORM_HEIGHT);
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
            const i = (y * width + x) * 4;
            const row = Math.round((1 - luma(data, i)) * (WAVEFORM_HEIGHT - 1));
            counts[row * width + x] += 1 / height;
        }
    }
    return phosphor(counts);
}

/**
 * A vectorscope: every pixel, as it shows over black, is plotted by its
 * blue-difference (horizontal) and red-difference (vertical, up) chroma, so
 * hue is the angle from the center and saturation the distance. Returns RGBA pixels of
 * {@link VECTORSCOPE_SIZE} squared.
 */
export function vectorscopePixels(frame: VideoPreviewFrame): Uint8ClampedArray {
    const { data, width, height } = frame;
    const counts = new Float32Array(VECTORSCOPE_SIZE * VECTORSCOPE_SIZE);
    const last = VECTORSCOPE_SIZE - 1;
    const weight = 1 / (width * height);
    for (let i = 0; i < width * height; i++) {
        const alpha = data[i * 4 + 3] / (255 * 255);
        const r = data[i * 4] * alpha;
        const g = data[i * 4 + 1] * alpha;
        const b = data[i * 4 + 2] * alpha;
        const cb = -0.168736 * r - 0.331264 * g + 0.5 * b;
        const cr = 0.5 * r - 0.418688 * g - 0.081312 * b;
        const x = Math.round((cb + 0.5) * last);
        const y = Math.round((0.5 - cr) * last);
        counts[y * VECTORSCOPE_SIZE + x] += weight;
    }
    return phosphor(counts);
}

/**
 * Draws `frame` into `canvas` in the requested view, resizing the canvas to
 * fit. The picture keeps its transparency over a checkerboard; the monitors
 * draw on black.
 */
export function drawPreview(
    canvas: HTMLCanvasElement,
    view: VideoPreviewView,
    frame: VideoPreviewFrame,
): void {
    const ctx = canvas.getContext('2d');
    if (ctx === null) return;

    let pixels: Uint8ClampedArray;
    let width = frame.width;
    let height = frame.height;
    if (view === 'waveform') {
        pixels = waveformPixels(frame);
        height = WAVEFORM_HEIGHT;
    } else if (view === 'vectorscope') {
        pixels = vectorscopePixels(frame);
        width = VECTORSCOPE_SIZE;
        height = VECTORSCOPE_SIZE;
    } else {
        pixels = new Uint8ClampedArray(
            frame.data.buffer,
            frame.data.byteOffset,
            frame.data.length,
        );
    }

    if (canvas.dataset.view !== view) {
        canvas.dataset.view = view;
        canvas.style.background = view === 'image' ? CHECKERBOARD : '#000';
    }
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    canvas.style.aspectRatio = `${width} / ${height}`;
    ctx.putImageData(
        new ImageData(new Uint8ClampedArray(pixels), width, height),
        0,
        0,
    );

    ctx.strokeStyle = 'rgba(255, 255, 255, 0.18)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    if (view === 'waveform') {
        for (let i = 0; i <= 4; i++) {
            const y = Math.round((i / 4) * (height - 1)) + 0.5;
            ctx.moveTo(0, y);
            ctx.lineTo(width, y);
        }
    } else if (view === 'vectorscope') {
        const center = width / 2;
        ctx.moveTo(center, 0);
        ctx.lineTo(center, height);
        ctx.moveTo(0, center);
        ctx.lineTo(width, center);
        ctx.moveTo(center + center * 0.9, center);
        ctx.arc(center, center, center * 0.9, 0, Math.PI * 2);
    }
    ctx.stroke();
}
