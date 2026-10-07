import { describe, expect, test, vi } from 'vitest';

import { applyButtonChange, applyGroupCollapse } from '../buttonChange';
import type { SliderEditModel } from '../sliderChange';
import type { CodeStyle } from '../../dsl/objectPropertyInsert';

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

const LAYOUT: CodeStyle = {
    bracketSpacing: true,
    printWidth: 80,
    quote: "'",
    tabWidth: 2,
    trailingComma: true,
    useTabs: false,
};

/** Collapse the last `$cGroup` call in `source` and return the new text. */
function collapseLast(
    source: string,
    collapsed: boolean,
    layout: CodeStyle = LAYOUT,
): string | null {
    const { model, pushEditOperations } = makeModel(source);
    applyGroupCollapse(
        { callStart: source.lastIndexOf('$cGroup') },
        collapsed,
        model,
        layout,
    );
    if (pushEditOperations.mock.calls.length === 0) {
        return null;
    }
    // makeModel maps an offset to column offset + 1 on a single line.
    const [edit] = pushEditOperations.mock.calls[0][1];
    return (
        source.slice(0, edit.range.startColumn - 1) +
        edit.text +
        source.slice(edit.range.endColumn - 1)
    );
}

describe('applyGroupCollapse', () => {
    test('replaces an existing collapsed literal', () => {
        expect(
            collapseLast("const g = $cGroup('Filter', { collapsed: false });", true),
        ).toBe("const g = $cGroup('Filter', { collapsed: true });");
    });

    test('appends a params object when the call has none', () => {
        expect(collapseLast("const g = $cGroup('Filter');", true)).toBe(
            "const g = $cGroup('Filter', { collapsed: true });",
        );
    });

    test('adds the property to a params object without one', () => {
        expect(
            collapseLast(
                "const p = $cGroup('P');\nconst g = $cGroup('Filter', { group: p });",
                true,
            ),
        ).toBe(
            "const p = $cGroup('P');\nconst g = $cGroup('Filter', { group: p, collapsed: true });",
        );
    });

    test('an added property follows the object layout', () => {
        expect(
            collapseLast(
                "const p = $cGroup('P');\nconst g = $cGroup('Filter', {\n  group: p,\n});",
                false,
            ),
        ).toBe(
            "const p = $cGroup('P');\nconst g = $cGroup('Filter', {\n  group: p,\n  collapsed: false,\n});",
        );
        expect(
            collapseLast("const g = $cGroup('A long group label');", true, {
                ...LAYOUT,
                printWidth: 40,
            }),
        ).toBe(
            "const g = $cGroup('A long group label', {\n  collapsed: true,\n});",
        );
    });

    test('an added property uses the source line ending', () => {
        expect(
            collapseLast(
                "const p = $cGroup('P');\r\nconst g = $cGroup('Filter', {\r\n  group: p,\r\n});",
                true,
            ),
        ).toBe(
            "const p = $cGroup('P');\r\nconst g = $cGroup('Filter', {\r\n  group: p,\r\n  collapsed: true,\r\n});",
        );
    });

    test('leaves a call whose collapsed state is not a literal alone', () => {
        expect(collapseLast("$cGroup('Filter', { collapsed: 1 });", true)).toBe(
            null,
        );
        expect(collapseLast("$cGroup('Filter', { collapsed });", true)).toBe(
            null,
        );
    });
});
