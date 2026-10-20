import { describe, expect, test } from 'vitest';

import type { ButtonView, SliderView } from '../controlBinding';
import { performanceControls } from '../performanceControls';

function slider(
    callStart: number,
    value: number,
    min: number,
    max: number,
): SliderView {
    return {
        argsStart: callStart + 8,
        callStart,
        group: [],
        incomplete: false,
        label: `s${callStart}`,
        max,
        min,
        moduleId: null,
        synced: false,
        unit: 'number',
        value,
        valueRange: { end: callStart + 20, start: callStart + 15 },
    };
}

function button(
    callStart: number,
    mode: 'gate' | 'toggle',
    value: boolean,
    moduleId: string | null,
): ButtonView {
    return {
        argsStart: callStart + 5,
        callStart,
        group: [],
        incomplete: false,
        label: `b${callStart}`,
        mode,
        moduleId,
        stateRange: null,
        synced: false,
        value,
    };
}

describe('performanceControls', () => {
    test('places sliders and buttons in call order, sliders by their place in the range', () => {
        expect(
            performanceControls(
                [slider(50, 0.75, 0, 1), slider(0, 5, 0, 10)],
                [button(20, 'toggle', true, null)],
                [],
            ),
        ).toEqual([
            { fraction: 0.5, kind: 'slider', offset: 0 },
            { kind: 'button', offset: 20, on: true },
            { fraction: 0.75, kind: 'slider', offset: 50 },
        ]);
    });

    test("a gate shows its running control's state", () => {
        const gates = [
            button(0, 'gate', false, 'held'),
            button(10, 'gate', false, 'released'),
            button(20, 'gate', false, null),
        ];
        const running = [
            {
                group: [],
                label: 'b0',
                mode: 'gate' as const,
                moduleId: 'held',
                value: true,
            },
            {
                group: [],
                label: 'b10',
                mode: 'gate' as const,
                moduleId: 'released',
                value: false,
            },
        ];
        expect(
            performanceControls([], gates, running).map((c) =>
                c.kind === 'button' ? c.on : null,
            ),
        ).toEqual([true, false, false]);
    });
});
