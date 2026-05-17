import type { SliderDefinition } from '../../../shared/dsl/sliderTypes';

type SignalFactory = (...args: unknown[]) => unknown;

/**
 * `$slider(label, value, min, max)` — create a UI-backed signal control.
 * Returns the signal output and registers a SliderDefinition for the renderer.
 */
export function create$slider(
    sliders: SliderDefinition[],
    signal: SignalFactory,
) {
    return (label: string, value: number, min: number, max: number) => {
        if (typeof label !== 'string') {
            throw new Error('$slider() label must be a string literal');
        }
        if (sliders.find((s) => s.label === label)) {
            throw new Error(`$slider() label "${label}" must be unique`);
        }
        if (typeof value !== 'number' || !isFinite(value)) {
            throw new Error('$slider() value must be a finite number literal');
        }
        if (typeof min !== 'number' || !isFinite(min)) {
            throw new Error('$slider() min must be a finite number');
        }
        if (typeof max !== 'number' || !isFinite(max)) {
            throw new Error('$slider() max must be a finite number');
        }
        if (min >= max) {
            throw new Error(
                `$slider() min (${min}) must be less than max (${max})`,
            );
        }

        const moduleId = `__slider_${label.replace(/[^a-zA-Z0-9_]/g, '_')}`;

        const result = signal(value, { id: moduleId });

        sliders.push({ label, max, min, moduleId, value });

        return result;
    };
}
