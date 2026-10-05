import { describe, expect, test } from 'vitest';

import { bindControls } from '../controlBinding';
import type { ButtonDefinition } from '../../../shared/dsl/buttonTypes';
import type { SliderDefinition } from '../../../shared/dsl/sliderTypes';
import type { ResolvedControls } from '../../dsl/extractControls';

/** Code controls at fixed call offsets: vol @0, hit @100, drone @200. */
const CODE: ResolvedControls = {
    groups: [],
    buttons: [
        {
            argsStart: 105,
            callStart: 100,
            group: [],
            incomplete: false,
            label: 'hit',
            mode: 'gate',
            stateRange: null,
            value: false,
        },
        {
            argsStart: 211,
            callStart: 200,
            group: [],
            incomplete: false,
            label: 'drone',
            mode: 'toggle',
            stateRange: { end: 225, start: 221 },
            value: true,
        },
    ],
    sliders: [
        {
            argsStart: 8,
            callStart: 0,
            group: [],
            incomplete: false,
            label: 'vol',
            max: 1,
            min: 0,
            unit: 'number',
            value: 0.5,
            valueRange: { end: 18, start: 15 },
        },
    ],
};

const RUNNING_SLIDERS: SliderDefinition[] = [
    {
        group: [],
        label: 'vol',
        max: 1,
        min: 0,
        moduleId: '__slider_vol',
        unit: 'number',
        value: 0.5,
    },
];

const RUNNING_BUTTONS: ButtonDefinition[] = [
    {
        group: [],
        label: 'hit',
        mode: 'gate',
        moduleId: '__button_hit',
        value: true,
    },
    {
        group: [],
        label: 'drone',
        mode: 'toggle',
        moduleId: '__button_drone',
        value: true,
    },
];

/** Anchors placing each running control at its CODE call offset. */
const ANCHORS: Record<string, number> = {
    __button_drone: 200,
    __button_hit: 100,
    __slider_vol: 0,
};
const anchored = (id: string) => ANCHORS[id] ?? null;
const unanchored = () => null;

describe('bindControls', () => {
    test('controls matching the running patch are live and synced', () => {
        const { sliders, buttons } = bindControls(
            CODE,
            RUNNING_SLIDERS,
            RUNNING_BUTTONS,
            true,
            anchored,
        );
        expect(sliders[0]).toMatchObject({
            moduleId: '__slider_vol',
            synced: true,
        });
        // A held gate is still synced: its value tracks the pointer.
        expect(buttons.map((b) => [b.moduleId, b.synced])).toEqual([
            ['__button_hit', true],
            ['__button_drone', true],
        ]);
    });

    test('a renamed label stays bound to its call through the anchor', () => {
        const code: ResolvedControls = {
            groups: [],
            buttons: [{ ...CODE.buttons[1], label: 'pad' }],
            sliders: [{ ...CODE.sliders[0], label: 'volume' }],
        };
        const { sliders, buttons } = bindControls(
            code,
            RUNNING_SLIDERS,
            RUNNING_BUTTONS,
            true,
            anchored,
        );
        // The label is for the user only, so a rename leaves it in sync.
        expect(sliders[0]).toMatchObject({
            label: 'volume',
            moduleId: '__slider_vol',
            synced: true,
        });
        expect(buttons[0]).toMatchObject({
            label: 'pad',
            moduleId: '__button_drone',
            synced: true,
        });
    });

    test('an anchored control binds by position, not by a matching label', () => {
        // A new call reuses the label 'vol' elsewhere; the anchored running
        // control stays with the call at its own offset.
        const code: ResolvedControls = {
            groups: [],
            buttons: [],
            sliders: [
                { ...CODE.sliders[0], label: 'renamed' },
                { ...CODE.sliders[0], argsStart: 58, callStart: 50 },
            ],
        };
        const { sliders } = bindControls(
            code,
            RUNNING_SLIDERS,
            [],
            true,
            anchored,
        );
        expect(sliders.map((s) => s.moduleId)).toEqual(['__slider_vol', null]);
    });

    test('without anchors, controls fall back to matching by label', () => {
        const { sliders, buttons } = bindControls(
            CODE,
            RUNNING_SLIDERS,
            RUNNING_BUTTONS,
            true,
            unanchored,
        );
        expect(sliders[0].moduleId).toBe('__slider_vol');
        expect(buttons.map((b) => b.moduleId)).toEqual([
            '__button_hit',
            '__button_drone',
        ]);
    });

    test('a buffer that is not running has no live controls', () => {
        const { sliders, buttons } = bindControls(
            CODE,
            RUNNING_SLIDERS,
            RUNNING_BUTTONS,
            false,
            anchored,
        );
        expect([...sliders, ...buttons].every((c) => c.moduleId === null)).toBe(
            true,
        );
        expect([...sliders, ...buttons].every((c) => !c.synced)).toBe(true);
    });

    test('an unevaluated control is neither live nor synced', () => {
        const code: ResolvedControls = {
            groups: [],
            buttons: [],
            sliders: [
                ...CODE.sliders,
                {
                    ...CODE.sliders[0],
                    argsStart: 58,
                    callStart: 50,
                    label: 'foo',
                },
            ],
        };
        const { sliders } = bindControls(
            code,
            RUNNING_SLIDERS,
            [],
            true,
            anchored,
        );
        expect(sliders[1]).toMatchObject({ moduleId: null, synced: false });
    });

    test('a live control whose code differs from the running patch is unsynced', () => {
        const code: ResolvedControls = {
            groups: [],
            buttons: [{ ...CODE.buttons[1], value: false }],
            sliders: [{ ...CODE.sliders[0], max: 2 }],
        };
        const { sliders, buttons } = bindControls(
            code,
            RUNNING_SLIDERS,
            RUNNING_BUTTONS,
            true,
            anchored,
        );
        expect(sliders[0]).toMatchObject({
            moduleId: '__slider_vol',
            synced: false,
        });
        expect(buttons[0]).toMatchObject({
            moduleId: '__button_drone',
            synced: false,
        });
    });

    test('a button whose mode changed in code is unsynced', () => {
        const code: ResolvedControls = {
            groups: [],
            buttons: [{ ...CODE.buttons[0], mode: 'toggle' }],
            sliders: [],
        };
        const { buttons } = bindControls(
            code,
            [],
            RUNNING_BUTTONS,
            true,
            unanchored,
        );
        expect(buttons[0].synced).toBe(false);
    });

    test('an incomplete control is neither live nor synced', () => {
        const code: ResolvedControls = {
            groups: [],
            buttons: [{ ...CODE.buttons[1], incomplete: true }],
            sliders: [{ ...CODE.sliders[0], incomplete: true }],
        };
        const { sliders, buttons } = bindControls(
            code,
            RUNNING_SLIDERS,
            RUNNING_BUTTONS,
            true,
            anchored,
        );
        expect(sliders[0]).toMatchObject({
            incomplete: true,
            moduleId: null,
            synced: false,
        });
        expect(buttons[0]).toMatchObject({
            incomplete: true,
            moduleId: null,
            synced: false,
        });
    });

    test('the label fallback only matches within the same group', () => {
        const code: ResolvedControls = {
            buttons: [],
            groups: [],
            sliders: [
                { ...CODE.sliders[0], group: ['A'] },
                {
                    ...CODE.sliders[0],
                    argsStart: 58,
                    callStart: 50,
                    group: ['B'],
                },
            ],
        };
        const running: SliderDefinition[] = [
            { ...RUNNING_SLIDERS[0], group: ['B'], moduleId: '__slider_B/vol' },
        ];
        const { sliders } = bindControls(code, running, [], true, unanchored);
        expect(sliders.map((s) => s.moduleId)).toEqual([
            null,
            '__slider_B/vol',
        ]);
    });
});
