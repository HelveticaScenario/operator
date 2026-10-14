import { describe, expect, it } from 'vitest';
import { alignWindow } from './alignWindow';

const sine = (n: number, period: number, phase = 0) =>
    Array.from({ length: n }, (_, i) =>
        Math.sin(((i + phase) / period) * Math.PI * 2),
    );

describe('alignWindow', () => {
    it('takes the newest samples without a trigger', () => {
        expect(
            Array.from(
                alignWindow([1, 2, 3, 4, 5], new Float32Array(3), false),
            ),
        ).toEqual([3, 4, 5]);
    });

    it('starts at a rising zero crossing', () => {
        const window = alignWindow(
            sine(200, 50, 17),
            new Float32Array(100),
            true,
        );
        expect(window[0]).toBeGreaterThanOrEqual(0);
        expect(window[0]).toBeLessThan(0.15);
        expect(window[1]).toBeGreaterThan(window[0]);
    });

    it('shows the same phase whatever the history phase', () => {
        const a = alignWindow(sine(400, 50, 3), new Float32Array(100), true);
        const b = alignWindow(sine(400, 50, 31), new Float32Array(100), true);
        expect(a[0]).toBeCloseTo(b[0], 1);
        expect(a[10]).toBeCloseTo(b[10], 1);
    });

    it('falls back to the newest samples when nothing crosses', () => {
        const flat = [0.5, 0.5, 0.5, 0.5, 0.5, 0.6];
        expect(
            Array.from(alignWindow(flat, new Float32Array(3), true)),
        ).toEqual([0.5, 0.5, 0.6].map(Math.fround));
    });

    it('ignores a crossing too late to leave a full window', () => {
        const late = [-1, -1, -1, -1, 1, 1];
        expect(
            Array.from(alignWindow(late, new Float32Array(4), true)),
        ).toEqual([-1, -1, 1, 1]);
    });

    it('pads with silence when there is less history than the window', () => {
        expect(
            Array.from(alignWindow([1, 2], new Float32Array(4), false)),
        ).toEqual([1, 2, 0, 0]);
    });

    it('overwrites every sample of a window it is handed again', () => {
        const into = new Float32Array([9, 9, 9, 9]);
        expect(alignWindow([1, 2], into, false)).toBe(into);
        expect(Array.from(into)).toEqual([1, 2, 0, 0]);
    });
});
