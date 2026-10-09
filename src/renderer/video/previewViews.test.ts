import { describe, expect, it } from 'vitest';
import type { VideoPreviewFrame } from '../../shared/video/videoGraph';
import {
    VECTORSCOPE_SIZE,
    WAVEFORM_HEIGHT,
    vectorscopePixels,
    waveformPixels,
} from './previewViews';

/** A frame filled by `color(x, y)`, returning r, g, b in 0..255. */
function frameOf(
    width: number,
    height: number,
    color: (x: number, y: number) => [number, number, number],
): VideoPreviewFrame {
    const data = new Uint8Array(width * height * 4);
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
            const [r, g, b] = color(x, y);
            data.set([r, g, b, 255], (y * width + x) * 4);
        }
    }
    return { data, height, index: 0, width };
}

const green = (
    pixels: Uint8ClampedArray,
    x: number,
    y: number,
    width: number,
) => pixels[(y * width + x) * 4 + 1];

describe('waveformPixels', () => {
    it('plots a uniform gray as a single line at its brightness', () => {
        const frame = frameOf(8, 8, () => [128, 128, 128]);
        const pixels = waveformPixels(frame);
        const row = Math.round((1 - 128 / 255) * (WAVEFORM_HEIGHT - 1));
        expect(green(pixels, 3, row, 8)).toBeGreaterThan(200);
        expect(green(pixels, 3, row + 5, 8)).toBe(0);
        expect(green(pixels, 3, 0, 8)).toBe(0);
    });

    it('puts white at the top and black at the bottom', () => {
        const frame = frameOf(2, 4, (x) =>
            x === 0 ? [255, 255, 255] : [0, 0, 0],
        );
        const pixels = waveformPixels(frame);
        expect(green(pixels, 0, 0, 2)).toBeGreaterThan(200);
        expect(green(pixels, 1, WAVEFORM_HEIGHT - 1, 2)).toBeGreaterThan(200);
    });

    it('traces a ramp as a diagonal', () => {
        const frame = frameOf(16, 4, (x) => {
            const v = Math.round((x / 15) * 255);
            return [v, v, v];
        });
        const pixels = waveformPixels(frame);
        const rowAt = (x: number) =>
            Array.from({ length: WAVEFORM_HEIGHT }, (_, y) => y).find(
                (y) => green(pixels, x, y, 16) > 100,
            )!;
        expect(rowAt(0)).toBeGreaterThan(rowAt(8));
        expect(rowAt(8)).toBeGreaterThan(rowAt(15));
    });

    it('returns opaque pixels of the frame width by the monitor height', () => {
        const pixels = waveformPixels(frameOf(5, 3, () => [0, 0, 0]));
        expect(pixels).toHaveLength(5 * WAVEFORM_HEIGHT * 4);
        expect(pixels[3]).toBe(255);
    });
});

describe('vectorscopePixels', () => {
    const at = (pixels: Uint8ClampedArray, x: number, y: number) =>
        green(pixels, x, y, VECTORSCOPE_SIZE);
    const center = Math.round((VECTORSCOPE_SIZE - 1) / 2);

    it('plots gray at the center', () => {
        const pixels = vectorscopePixels(frameOf(6, 6, () => [100, 100, 100]));
        expect(at(pixels, center, center)).toBeGreaterThan(200);
    });

    it('plots red above the center and blue to its right', () => {
        const red = vectorscopePixels(frameOf(6, 6, () => [255, 0, 0]));
        const blue = vectorscopePixels(frameOf(6, 6, () => [0, 0, 255]));
        const redSpot = findLit(red);
        const blueSpot = findLit(blue);
        expect(redSpot.y).toBeLessThan(center);
        expect(blueSpot.x).toBeGreaterThan(center);
        expect(blueSpot.y).toBeGreaterThanOrEqual(center);
    });

    it('spreads a gradient of hues across the scope', () => {
        const frame = frameOf(8, 1, (x) => (x < 4 ? [255, 0, 0] : [0, 255, 0]));
        const lit = litCount(vectorscopePixels(frame));
        expect(lit).toBe(2);
    });
});

function findLit(pixels: Uint8ClampedArray): { x: number; y: number } {
    for (let y = 0; y < VECTORSCOPE_SIZE; y++) {
        for (let x = 0; x < VECTORSCOPE_SIZE; x++) {
            if (green(pixels, x, y, VECTORSCOPE_SIZE) > 100) return { x, y };
        }
    }
    throw new Error('nothing plotted');
}

function litCount(pixels: Uint8ClampedArray): number {
    let count = 0;
    for (let i = 1; i < pixels.length; i += 4) if (pixels[i] > 0) count++;
    return count;
}
