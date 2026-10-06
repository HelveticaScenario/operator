import { describe, expect, test } from 'vitest';

import { hzToVolts } from '../../../shared/dsl/sliderUnits';
import {
    positionToVolts,
    sliderPositionCount,
    voltsToPosition,
} from '../sliderPositions';

const HZ = { max: hzToVolts(2000), min: hzToVolts(100), unit: 'hz' as const };
const NOTE = { max: 2, min: -23 / 12, unit: 'note' as const }; // c#2..c6

describe('slider positions', () => {
    test('the end positions are exactly min and max', () => {
        for (const slider of [HZ, NOTE]) {
            expect(positionToVolts(0, slider)).toBe(slider.min);
            expect(positionToVolts(sliderPositionCount(slider), slider)).toBe(
                slider.max,
            );
        }
    });

    test('a note slider has one position per semitone', () => {
        expect(sliderPositionCount(NOTE)).toBe(47);
        expect(positionToVolts(1, NOTE)).toBe(-22 / 12);
    });

    test('volts map back to their position, clamped to the range', () => {
        expect(voltsToPosition(HZ.max, HZ)).toBe(1000);
        expect(voltsToPosition(HZ.min, HZ)).toBe(0);
        expect(voltsToPosition(HZ.max + 1, HZ)).toBe(1000);
        expect(voltsToPosition(HZ.min - 1, HZ)).toBe(0);
        for (const position of [0, 1, 250, 999, 1000]) {
            expect(voltsToPosition(positionToVolts(position, HZ), HZ)).toBe(
                position,
            );
        }
    });
});
