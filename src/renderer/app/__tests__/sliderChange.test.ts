import { describe, expect, test, vi } from 'vitest';

import { applySliderChange, type SliderEditModel } from '../sliderChange';
import type { SliderDefinition } from '../../../shared/dsl/sliderTypes';

const SLIDER: SliderDefinition = {
    label: 'cutoff',
    max: 1,
    min: 0,
    moduleId: 'signal-1',
    value: 0.5,
};

const SOURCE = "$slider('cutoff', 0.5, 0, 1);\n";

function makeModel(source: string) {
    const pushEditOperations = vi.fn();
    const model: SliderEditModel = {
        getPositionAt: (offset: number) => ({
            column: offset + 1,
            lineNumber: 1,
        }),
        getValue: () => source,
        pushEditOperations,
    };
    return { model, pushEditOperations };
}

describe('applySliderChange', () => {
    test('edits the document when the active buffer is the running buffer', () => {
        const { model, pushEditOperations } = makeModel(SOURCE);
        const setModuleParam = vi.fn();

        applySliderChange(SLIDER, 0.75, model, true, setModuleParam);

        expect(setModuleParam).toHaveBeenCalledWith('signal-1', '$signal', {
            source: 0.75,
        });
        expect(pushEditOperations).toHaveBeenCalledTimes(1);
        const [, edits] = pushEditOperations.mock.calls[0];
        expect(edits[0].text).toBe('0.75');
    });

    test('never edits the document when a different buffer is active, even one containing a matching $slider call', () => {
        const { model, pushEditOperations } = makeModel(SOURCE);
        const setModuleParam = vi.fn();

        applySliderChange(SLIDER, 0.75, model, false, setModuleParam);

        // The engine still receives the value; the visible document does not.
        expect(setModuleParam).toHaveBeenCalledWith('signal-1', '$signal', {
            source: 0.75,
        });
        expect(pushEditOperations).not.toHaveBeenCalled();
    });

    test('updates the engine even when no editor model is available', () => {
        const setModuleParam = vi.fn();

        applySliderChange(SLIDER, 0.25, null, true, setModuleParam);

        expect(setModuleParam).toHaveBeenCalledWith('signal-1', '$signal', {
            source: 0.25,
        });
    });
});
