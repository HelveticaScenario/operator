import { describe, expect, it, vi } from 'vitest';

vi.mock('electron', () => ({
    app: { commandLine: { hasSwitch: () => false } },
    desktopCapturer: {},
    screen: {},
    systemPreferences: {},
}));

const { pickScreenSource } = await import('../liveSources');

const displays = [{ id: 11 }, { id: 22 }];
const sources = [
    { display_id: '22', id: 'screen:1:0' },
    { display_id: '11', id: 'screen:0:0' },
];

describe('pickScreenSource', () => {
    it('finds the source of the display by its id, whatever order sources come in', () => {
        expect(pickScreenSource(sources, displays, 1)).toEqual({
            id: 'screen:0:0',
        });
        expect(pickScreenSource(sources, displays, 2)).toEqual({
            id: 'screen:1:0',
        });
    });

    it('falls back on position when no source names the display', () => {
        const unnamed = [
            { display_id: '', id: 'first' },
            { display_id: '', id: 'second' },
        ];
        expect(pickScreenSource(unnamed, displays, 2)).toEqual({
            id: 'second',
        });
    });

    it('says how many displays there are when the number is too big', () => {
        expect(pickScreenSource(sources, displays, 3)).toEqual({
            error: 'there is no display 3; there are 2 displays',
        });
        expect(pickScreenSource(sources, [{ id: 1 }], 2)).toEqual({
            error: 'there is no display 2; there is 1 display',
        });
    });

    it('reports a display that has no source', () => {
        expect(pickScreenSource([], displays, 1)).toEqual({
            error: 'display 1 cannot be captured',
        });
    });
});
