import { describe, expect, it } from 'vitest';

import {
    DEFAULT_Q,
    type EqSectionValues,
    constantSignalValue,
    hzToVoct,
    quantizeGain,
    quantizeHz,
    quantizeQ,
    sectionResponseDb,
    voctToHz,
} from './eqMath';

const SR = 48000;

function responseAt(sec: EqSectionValues, hz: number, adaptive = false) {
    const out = new Float64Array(1);
    sectionResponseDb(sec, adaptive, SR, [hz], out);
    return out[0];
}

describe('constantSignalValue', () => {
    it('decodes numbers, hz strings, and note names to V/Oct', () => {
        expect(constantSignalValue(0.5)).toBe(0.5);
        expect(constantSignalValue('261.6255653005986hz')).toBeCloseTo(0, 9);
        expect(constantSignalValue('523.2511306011972HZ')).toBeCloseTo(1, 9);
        expect(constantSignalValue('c4')).toBe(0);
        expect(constantSignalValue('A4')).toBeCloseTo(0.75, 9);
        expect(constantSignalValue('c#3')).toBeCloseTo(-11 / 12, 9);
        expect(constantSignalValue('Bb')).toBeCloseTo(10 / 12, 9);
        expect(constantSignalValue([2])).toBe(2);
        expect(constantSignalValue([1, 'c5'])).toBe(2);
    });

    it('rejects signal-driven and invalid values', () => {
        expect(constantSignalValue({ module: 'x', port: 'output' })).toBeNull();
        expect(constantSignalValue([1, { module: 'x' }])).toBeNull();
        expect(constantSignalValue([])).toBeNull();
        expect(constantSignalValue('-10hz')).toBeNull();
        expect(constantSignalValue('nope')).toBeNull();
    });
});

describe('quantizers', () => {
    it('round to the precision the panel writes', () => {
        expect(quantizeHz(57.34)).toBe(57.3);
        expect(quantizeHz(1234.6)).toBe(1235);
        expect(quantizeHz(5)).toBe(20);
        expect(quantizeGain(-0.04)).toBe(0);
        expect(quantizeGain(30)).toBe(24);
        expect(quantizeQ(0.7071)).toBe(0.71);
        expect(quantizeQ(12.4)).toBe(12);
    });

    it('round-trips V/Oct', () => {
        expect(voctToHz(hzToVoct(1234))).toBeCloseTo(1234, 9);
    });
});

describe('sectionResponseDb', () => {
    const at = (type: EqSectionValues['type'], gainDb: number, q = 1) => ({
        freqHz: 1000,
        gainDb,
        q,
        type,
    });

    it('peaks at the set gain at the center frequency', () => {
        expect(responseAt(at('peak', 12), 1000)).toBeCloseTo(12, 6);
        expect(responseAt(at('peak', -9), 1000)).toBeCloseTo(-9, 6);
        expect(Math.abs(responseAt(at('peak', 12), 20))).toBeLessThan(0.1);
    });

    it('keeps the peak gain under adaptive Q while narrowing the bell', () => {
        expect(responseAt(at('peak', 12), 1000, true)).toBeCloseTo(12, 6);
        expect(responseAt(at('peak', 12), 2000, true)).toBeLessThan(
            responseAt(at('peak', 12), 2000, false),
        );
    });

    it('shelves reach the set gain on their side and unity on the other', () => {
        expect(responseAt(at('lowShelf', 6, DEFAULT_Q), 20)).toBeCloseTo(6, 1);
        expect(responseAt(at('lowShelf', 6, DEFAULT_Q), 20000)).toBeCloseTo(
            0,
            1,
        );
        expect(responseAt(at('highShelf', -6, DEFAULT_Q), 20000)).toBeCloseTo(
            -6,
            0,
        );
        expect(responseAt(at('highShelf', -6, DEFAULT_Q), 20)).toBeCloseTo(
            0,
            1,
        );
        // Half the shelf gain at the corner.
        expect(responseAt(at('lowShelf', 6, DEFAULT_Q), 1000)).toBeCloseTo(
            3,
            6,
        );
    });

    it('shelves overshoot their corner only above Q 0.707', () => {
        const sweep = (q: number) =>
            Array.from({ length: 41 }, (_, i) =>
                responseAt(
                    at('lowShelf', 12, q),
                    1000 * Math.pow(2, i / 10 - 2),
                ),
            );
        const steep = sweep(DEFAULT_Q);
        expect(Math.max(...steep)).toBeLessThanOrEqual(12.01);
        expect(Math.min(...steep)).toBeGreaterThanOrEqual(-0.01);
        const resonant = sweep(4);
        expect(Math.max(...resonant)).toBeGreaterThan(13);
        expect(Math.min(...resonant)).toBeLessThan(-1);
    });

    it('cut filters pass one side, ignore gain, and are -3 dB at Q 0.707', () => {
        expect(responseAt(at('lowpass', 12, DEFAULT_Q), 1000)).toBeCloseTo(
            -3.01,
            1,
        );
        expect(responseAt(at('lowpass', 0, DEFAULT_Q), 50)).toBeCloseTo(0, 2);
        expect(responseAt(at('highpass', 0, DEFAULT_Q), 1000)).toBeCloseTo(
            -3.01,
            1,
        );
        expect(responseAt(at('highpass', 0, DEFAULT_Q), 15000)).toBeCloseTo(
            0,
            1,
        );
        expect(responseAt(at('notch', 0, 4), 1000)).toBeLessThan(-60);
    });
});
