import { describe, expect, test } from 'vitest';

import {
    formatHzLiteral,
    hzToVolts,
    parseSliderValue,
    snapVoltsToSemitone,
    voltsToHz,
    voltsToNoteName,
} from '../sliderUnits';

const C4_HZ = 261.6255653005986;

describe('parseSliderValue', () => {
    test('passes finite numbers through as volts', () => {
        expect(parseSliderValue(2.5)).toEqual({ unit: 'number', volts: 2.5 });
        expect(parseSliderValue(-1)).toEqual({ unit: 'number', volts: -1 });
    });

    test('rejects non-finite numbers', () => {
        expect(() => parseSliderValue(Infinity)).toThrow('finite');
        expect(() => parseSliderValue(NaN)).toThrow('finite');
    });

    test('parses hz strings case-insensitively', () => {
        for (const s of ['440hz', '440Hz', '440HZ']) {
            const parsed = parseSliderValue(s);
            expect(parsed.unit).toBe('hz');
            expect(parsed.volts).toBeCloseTo(Math.log2(440 / C4_HZ), 10);
        }
    });

    test('rejects non-positive hz', () => {
        expect(() => parseSliderValue('0hz')).toThrow('positive');
        expect(() => parseSliderValue('-5hz')).toThrow('positive');
    });

    test('parses note strings with sharps, flats, and case folding', () => {
        expect(parseSliderValue('c4')).toEqual({ unit: 'note', volts: 0 });
        expect(parseSliderValue('C4')).toEqual({ unit: 'note', volts: 0 });
        expect(parseSliderValue('a#3').volts).toBeCloseTo((58 - 60) / 12, 10);
        expect(parseSliderValue('db4').volts).toBeCloseTo(1 / 12, 10);
    });

    test('bare note letters default to octave 4', () => {
        expect(parseSliderValue('a').volts).toBeCloseTo(9 / 12, 10);
        expect(parseSliderValue('c#').volts).toBeCloseTo(1 / 12, 10);
    });

    test('rejects garbage strings', () => {
        for (const s of ['h4', '440 hz', 'hz', '']) {
            expect(() => parseSliderValue(s)).toThrow();
        }
    });

    test('rejects uppercase flat accidentals like the Rust note grammar', () => {
        expect(() => parseSliderValue('AB3')).toThrow('invalid slider value');
        expect(() => parseSliderValue('aB3')).toThrow('invalid slider value');
    });
});

describe('round trips', () => {
    test('note names re-parse to exactly the snapped volts', () => {
        for (let i = 0; i <= 100; i++) {
            const volts = -3 + (6 * i) / 100;
            const name = voltsToNoteName(volts);
            expect(parseSliderValue(name).volts).toBe(
                snapVoltsToSemitone(volts),
            );
        }
    });

    test('hz literals re-parse to nearly identical volts', () => {
        for (let i = 0; i <= 100; i++) {
            const volts = -4 + (8 * i) / 100;
            const literal = formatHzLiteral(voltsToHz(volts));
            expect(parseSliderValue(literal).volts).toBeCloseTo(volts, 4);
        }
    });

    test('hzToVolts and voltsToHz are inverses', () => {
        expect(voltsToHz(hzToVolts(440))).toBeCloseTo(440, 8);
        expect(hzToVolts(C4_HZ)).toBeCloseTo(0, 10);
    });
});

describe('formatHzLiteral', () => {
    test('never emits exponent notation', () => {
        const HZ_RE = /^(-?\d*\.?\d+)hz$/i;
        for (const hz of [0.001, 0.5, 27.5, 440, 12543.85, 20000]) {
            expect(formatHzLiteral(hz)).toMatch(HZ_RE);
        }
    });

    test('extreme magnitudes re-parse to a positive frequency', () => {
        for (const hz of [1e-7, 2.5e-7, 6.25e-10, 1.23e22, 1e21]) {
            const literal = formatHzLiteral(hz);
            const parsed = parseSliderValue(literal);
            expect(parsed.unit).toBe('hz');
            expect(voltsToHz(parsed.volts) / hz).toBeCloseTo(1, 4);
        }
    });

    test('sub-microhertz values keep their significant digits', () => {
        expect(formatHzLiteral(1e-7)).toBe('0.0000001hz');
        expect(formatHzLiteral(2.5e-7)).toBe('0.00000025hz');
    });
});
