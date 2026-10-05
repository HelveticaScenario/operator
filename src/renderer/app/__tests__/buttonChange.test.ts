import { describe, expect, test, vi } from 'vitest';

import { applyButtonChange, applyGroupCollapse } from '../buttonChange';
import type { SliderEditModel } from '../sliderChange';

const TOGGLE = { callStart: 0, mode: 'toggle' as const };
const GATE = { callStart: 0, mode: 'gate' as const };

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
    test('a live toggle drives the gate voltage and rewrites its state literal', () => {
        const { model, pushEditOperations } = makeModel(
            "$toggleBtn('drone', false).out();",
        );
        const setModuleParam = vi.fn();

        applyButtonChange(
            TOGGLE,
            true,
            '__button_drone',
            model,
            setModuleParam,
        );

        expect(setModuleParam).toHaveBeenCalledWith(
            '__button_drone',
            '$signal',
            { source: 5 },
        );
        expect(pushEditOperations).toHaveBeenCalledTimes(1);
        expect(pushEditOperations.mock.calls[0][1][0].text).toBe('true');
    });

    test('a toggle not in the running patch rewrites the code but leaves the engine alone', () => {
        const { model, pushEditOperations } = makeModel(
            "$toggleBtn('drone', false).out();",
        );
        const setModuleParam = vi.fn();

        applyButtonChange(TOGGLE, true, null, model, setModuleParam);

        expect(setModuleParam).not.toHaveBeenCalled();
        expect(pushEditOperations.mock.calls[0][1][0].text).toBe('true');
    });

    test('a gate drives the engine but never touches the source', () => {
        const { model, pushEditOperations } = makeModel("$btn('hit').out();");
        const setModuleParam = vi.fn();

        applyButtonChange(GATE, true, '__button_hit', model, setModuleParam);
        applyButtonChange(GATE, false, '__button_hit', model, setModuleParam);

        expect(setModuleParam.mock.calls.map((c) => c[2])).toEqual([
            { source: 5 },
            { source: 0 },
        ]);
        expect(pushEditOperations).not.toHaveBeenCalled();
    });
});

describe('applyGroupCollapse', () => {
    test('replaces an existing collapsed-state literal', () => {
        const { model, pushEditOperations } = makeModel(
            "const g = $cGroup('Filter', false);",
        );
        applyGroupCollapse({ callStart: 10 }, true, model);
        const [edit] = pushEditOperations.mock.calls[0][1];
        expect(edit.text).toBe('true');
        expect(edit.range.startColumn).toBe(
            "const g = $cGroup('Filter', ".length + 1,
        );
    });

    test('appends the state after the label when the call has none', () => {
        const source = "const g = $cGroup('Filter');";
        const { model, pushEditOperations } = makeModel(source);
        applyGroupCollapse({ callStart: 10 }, true, model);
        const [edit] = pushEditOperations.mock.calls[0][1];
        expect(edit.text).toBe(', true');
        const at = source.indexOf(')') + 1;
        expect([edit.range.startColumn, edit.range.endColumn]).toEqual([
            at,
            at,
        ]);
    });

    test('leaves a call with invalid arguments alone', () => {
        const { model, pushEditOperations } = makeModel(
            "const g = $cGroup('Filter', 1);",
        );
        applyGroupCollapse({ callStart: 10 }, true, model);
        expect(pushEditOperations).not.toHaveBeenCalled();
    });
});
