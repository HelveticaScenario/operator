import { describe, expect, it } from 'vitest';
import type { VideoPreviewFrame } from '../../shared/video/videoGraph';
import { regionAverage } from './cvSample';

/** A frame whose pixel (x, y) has brightness `level(x, y)` in 0..1. */
function frameOf(
    width: number,
    height: number,
    level: (x: number, y: number) => number,
): VideoPreviewFrame {
    const data = new Uint8Array(width * height * 4);
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
            const v = Math.round(level(x, y) * 255);
            data.set([v, v, v, 255], (y * width + x) * 4);
        }
    }
    return { data, height, index: 0, width };
}

const whole = { size: 0.5, x: 0.5, y: 0.5 };

describe('regionAverage', () => {
    it('averages the whole frame by default region', () => {
        const frame = frameOf(10, 10, (x) => (x < 5 ? 0 : 1));
        expect(regionAverage(frame, whole)).toBeCloseTo(0.5, 2);
    });

    it('reads a small region around its center', () => {
        const frame = frameOf(100, 100, (x) => x / 99);
        expect(
            regionAverage(frame, { size: 0.02, x: 0.25, y: 0.5 }),
        ).toBeCloseTo(0.25, 1);
        expect(
            regionAverage(frame, { size: 0.02, x: 0.9, y: 0.5 }),
        ).toBeCloseTo(0.9, 1);
    });

    it('measures y up from the bottom of the frame', () => {
        const frame = frameOf(10, 10, (_x, y) => (y < 5 ? 1 : 0));
        expect(regionAverage(frame, { size: 0.1, x: 0.5, y: 0.9 })).toBeCloseTo(
            1,
            2,
        );
        expect(regionAverage(frame, { size: 0.1, x: 0.5, y: 0.1 })).toBeCloseTo(
            0,
            2,
        );
    });

    it('weights red, green and blue by perceived brightness', () => {
        const data = new Uint8Array([255, 0, 0, 255]);
        const red = { data, height: 1, index: 0, width: 1 };
        expect(regionAverage(red, whole)).toBeCloseTo(0.299, 3);
    });

    it('clips a region that hangs off the frame', () => {
        const frame = frameOf(10, 10, () => 0.6);
        expect(regionAverage(frame, { size: 0.3, x: 0, y: 0 })).toBeCloseTo(
            0.6,
            2,
        );
    });

    it('covers at least one pixel however small the region', () => {
        const frame = frameOf(10, 10, () => 0.8);
        expect(
            regionAverage(frame, { size: 0.0001, x: 0.5, y: 0.5 }),
        ).toBeCloseTo(0.8, 2);
    });
});
