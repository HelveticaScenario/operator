import { describe, expect, it } from 'vitest';
import { scrollEaseMs } from './settings';

describe('scrollEaseMs', () => {
    it('runs from 1 s at the slowest to 10 ms just short of instant', () => {
        expect(scrollEaseMs(0)).toBeCloseTo(1000);
        expect(scrollEaseMs(0.5)).toBeCloseTo(100);
        expect(scrollEaseMs(0.999)).toBeCloseTo(10, 0);
    });

    it('jumps at the fastest', () => {
        expect(scrollEaseMs(1)).toBe(0);
    });

    it('gets faster as the speed rises', () => {
        expect(scrollEaseMs(0.3)).toBeGreaterThan(scrollEaseMs(0.7));
    });
});
