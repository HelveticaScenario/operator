import { describe, expect, test, vi } from 'vitest';

import { applySliderChange, type SliderEditModel } from '../sliderChange';

const SLIDER = { callStart: 0, unit: 'number' as const };

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
    test('a live slider updates both the engine and the code', () => {
        const { model, pushEditOperations } = makeModel(SOURCE);
        const setModuleParam = vi.fn();

        applySliderChange(SLIDER, 0.75, 'signal-1', model, setModuleParam);

        expect(setModuleParam).toHaveBeenCalledWith('signal-1', '$signal', {
            source: 0.75,
        });
        expect(pushEditOperations).toHaveBeenCalledTimes(1);
        const [, edits] = pushEditOperations.mock.calls[0];
        expect(edits[0].text).toBe('0.75');
    });

    test('a slider not in the running patch edits the code but leaves the engine alone', () => {
        const { model, pushEditOperations } = makeModel(SOURCE);
        const setModuleParam = vi.fn();

        applySliderChange(SLIDER, 0.75, null, model, setModuleParam);

        expect(setModuleParam).not.toHaveBeenCalled();
        expect(pushEditOperations).toHaveBeenCalledTimes(1);
        expect(pushEditOperations.mock.calls[0][1][0].text).toBe('0.75');
    });

    test('updates the engine even when no editor model is available', () => {
        const setModuleParam = vi.fn();

        applySliderChange(SLIDER, 0.25, 'signal-1', null, setModuleParam);

        expect(setModuleParam).toHaveBeenCalledWith('signal-1', '$signal', {
            source: 0.25,
        });
    });

    test('the engine receives exactly the value the rewritten literal parses to', () => {
        const { model } = makeModel(SOURCE);
        const setModuleParam = vi.fn();

        const written = applySliderChange(
            SLIDER,
            1 / 3,
            'signal-1',
            model,
            setModuleParam,
        );

        expect(written).toBe(0.333333);
        expect(setModuleParam.mock.calls[0][2]).toEqual({ source: 0.333333 });
    });

    test('writes hz and note values back in their own unit and quote style', () => {
        const hzModel = makeModel(
            `$slider("pitch", "440hz", "55hz", "1760hz");`,
        );
        // 0 V/Oct is C4.
        applySliderChange(
            { callStart: 0, unit: 'hz' as const },
            0,
            null,
            hzModel.model,
            vi.fn(),
        );
        expect(hzModel.pushEditOperations.mock.calls[0][1][0].text).toBe(
            '"261.626hz"',
        );

        const noteModel = makeModel(`$slider('root', 'c4', 'c2', 'c6');`);
        const written = applySliderChange(
            { callStart: 0, unit: 'note' as const },
            9 / 12 + 0.01,
            null,
            noteModel.model,
            vi.fn(),
        );
        expect(noteModel.pushEditOperations.mock.calls[0][1][0].text).toBe(
            "'a4'",
        );
        expect(written).toBe(9 / 12);
    });

    test('finds the slider by call position, not label', () => {
        // Two groups each hold a 'cutoff'; only the call at the given offset
        // is rewritten.
        const source =
            "$cGroup('A').slider('cutoff', 1, 0, 2);\n" +
            "$cGroup('B').slider('cutoff', 1, 0, 2);\n";
        const { model, pushEditOperations } = makeModel(source);
        const second = source.lastIndexOf('.slider') + 1;

        applySliderChange(
            { callStart: second, unit: 'number' },
            1.5,
            null,
            model,
            vi.fn(),
        );

        const [, edits] = pushEditOperations.mock.calls[0];
        const valueAt = source.indexOf('1', second);
        expect(edits[0].range.startColumn).toBe(valueAt + 1);
        expect(edits[0].text).toBe('1.5');
    });
});
