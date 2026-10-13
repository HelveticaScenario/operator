import { describe, expect, it } from 'vitest';

import { octavesToQ, qToOctaves } from './eqGeometry';
import { decodeSection, withLive } from './eqPanelModel';

const cable = { channel: 0, module: 'lfo', port: 'output', type: 'cable' };

describe('decodeSection', () => {
    it('marks signal-driven params and leaves their values unknown', () => {
        const sec = decodeSection({ freq: cable, gain: 3, q: [cable] });
        expect(sec.driven).toEqual({ freq: true, gain: false, q: true });
        expect(sec.freqHz).toBeNull();
        expect(sec.gainDb).toBe(3);
        expect(sec.q).toBeNull();
    });

    it('treats absent optional params as constant defaults', () => {
        const sec = decodeSection({ freq: '1000hz' });
        expect(sec.driven).toEqual({ freq: false, gain: false, q: false });
        expect(sec.gainDb).toBe(0);
        expect(sec.q).toBeCloseTo(Math.SQRT1_2, 9);
    });
});

describe('withLive', () => {
    it('fills only the driven params from the engine values', () => {
        const sec = decodeSection({ freq: cable, gain: 3 });
        const shown = withLive(sec, [1, -10, 4]);
        expect(shown.freqHz).toBeCloseTo(523.25, 2);
        expect(shown.gainDb).toBe(3);
        expect(shown.q).toBeCloseTo(Math.SQRT1_2, 9);
    });

    it('keeps unknown values unknown without live data', () => {
        expect(withLive(decodeSection({ freq: cable }), undefined).freqHz).toBe(
            null,
        );
    });
});

describe('Q handle mapping', () => {
    it('maps Q to peak bandwidth in octaves and back', () => {
        expect(qToOctaves(Math.SQRT2)).toBeCloseTo(1, 6);
        for (const q of [0.1, 0.71, 2, 20]) {
            expect(octavesToQ(qToOctaves(q))).toBeCloseTo(q, 9);
        }
    });
});
