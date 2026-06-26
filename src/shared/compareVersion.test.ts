import { describe, expect, it } from 'vitest';
import { compareVersion } from './compareVersion';

const sign = (n: number) => (n < 0 ? -1 : n > 0 ? 1 : 0);

describe('compareVersion', () => {
    it('orders by major, then minor, then patch', () => {
        expect(sign(compareVersion('0.0.101', '0.0.102'))).toBe(-1);
        expect(sign(compareVersion('0.1.0', '0.0.99'))).toBe(1);
        expect(sign(compareVersion('1.0.0', '0.9.9'))).toBe(1);
        expect(sign(compareVersion('0.0.68', '0.0.68'))).toBe(0);
    });

    it('treats missing components as zero', () => {
        expect(sign(compareVersion('0.1', '0.1.0'))).toBe(0);
        expect(sign(compareVersion('1', '1.0.0'))).toBe(0);
        expect(sign(compareVersion('0.1', '0.1.1'))).toBe(-1);
    });

    it('does not NaN-out on a pre-release suffix (compares the core)', () => {
        // The old inline comparator went all-false on these; the core must win.
        expect(sign(compareVersion('1.2.0-beta.1', '1.2.0'))).toBe(0);
        expect(sign(compareVersion('1.2.0-beta.1', '1.1.0'))).toBe(1);
    });
});
