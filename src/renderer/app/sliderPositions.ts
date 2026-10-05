import type { SliderUnit } from '../../shared/dsl/sliderUnits';
import { snapVoltsToSemitone } from '../../shared/dsl/sliderUnits';

/**
 * A slider's range input runs over whole-number positions 0..N, mapped to
 * volts here. The browser applies `step` to the decimal text of the
 * `min`/`max`/`step` attributes, so a fractional volt step can round the
 * position count down and leave `max` unreachable; integer positions cannot.
 */
interface SliderRange {
    min: number;
    max: number;
    unit: SliderUnit;
}

/** Positions for a continuous (number / hz) slider. */
const CONTINUOUS_POSITIONS = 1000;

/** Number of steps between min and max: semitones for a note slider. */
export function sliderPositionCount(slider: SliderRange): number {
    return slider.unit === 'note'
        ? Math.max(1, Math.round((slider.max - slider.min) * 12))
        : CONTINUOUS_POSITIONS;
}

/** Volts at a position; the last position is exactly `max`. */
export function positionToVolts(position: number, slider: SliderRange): number {
    const count = sliderPositionCount(slider);
    if (position >= count) {
        return slider.max;
    }
    if (position <= 0) {
        return slider.min;
    }
    return slider.unit === 'note'
        ? snapVoltsToSemitone(slider.min + position / 12)
        : slider.min + ((slider.max - slider.min) * position) / count;
}

/** Nearest position to a volts value, clamped to the range. */
export function voltsToPosition(volts: number, slider: SliderRange): number {
    const count = sliderPositionCount(slider);
    const position = Math.round(
        ((volts - slider.min) / (slider.max - slider.min)) * count,
    );
    return Math.min(count, Math.max(0, position));
}
