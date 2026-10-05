import { describe, expect, test, vi } from 'vitest';

import { applyButtonChange } from '../buttonChange';
import type { SliderEditModel } from '../sliderChange';
import type { ButtonDefinition } from '../../../shared/dsl/buttonTypes';

const TOGGLE: ButtonDefinition = {
    label: 'drone',
    mode: 'toggle',
    moduleId: '__button_drone',
    value: false,
};

const GATE: ButtonDefinition = {
    label: 'hit',
    mode: 'gate',
    moduleId: '__button_hit',
    value: false,
};

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

describe('applyButtonChange', () => {
    test('a toggle drives the gate voltage and rewrites its state literal', () => {
        const { model, pushEditOperations } = makeModel(
            "$toggleBtn('drone', false).out();",
        );
        const setModuleParam = vi.fn();

        applyButtonChange(TOGGLE, true, model, true, setModuleParam);

        expect(setModuleParam).toHaveBeenCalledWith(
            '__button_drone',
            '$signal',
            { source: 5 },
        );
        expect(pushEditOperations).toHaveBeenCalledTimes(1);
        expect(pushEditOperations.mock.calls[0][1][0].text).toBe('true');
    });

    test('a toggle never edits a buffer other than the running one', () => {
        const { model, pushEditOperations } = makeModel(
            "$toggleBtn('drone', false).out();",
        );
        const setModuleParam = vi.fn();

        applyButtonChange(TOGGLE, true, model, false, setModuleParam);

        expect(setModuleParam).toHaveBeenCalledWith(
            '__button_drone',
            '$signal',
            { source: 5 },
        );
        expect(pushEditOperations).not.toHaveBeenCalled();
    });

    test('a gate drives the engine but never touches the source', () => {
        const { model, pushEditOperations } = makeModel("$btn('hit').out();");
        const setModuleParam = vi.fn();

        applyButtonChange(GATE, true, model, true, setModuleParam);
        applyButtonChange(GATE, false, model, true, setModuleParam);

        expect(setModuleParam.mock.calls.map((c) => c[2])).toEqual([
            { source: 5 },
            { source: 0 },
        ]);
        expect(pushEditOperations).not.toHaveBeenCalled();
    });
});
