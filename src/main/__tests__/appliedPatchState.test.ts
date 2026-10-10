import type { PatchGraph } from '@modular/core';
import { describe, expect, test } from 'vitest';
import { AppliedPatchState } from '../appliedPatchState';

const graph = (name: string) => ({ name }) as unknown as PatchGraph;

/** Submit an update the way main does: against the current baseline. */
function submit(
    state: AppliedPatchState,
    name: string,
    sourceId: string,
    updateId: number,
    lastAppliedUpdateId: number,
): void {
    state.record(
        graph(name),
        sourceId,
        updateId,
        state.baseline(lastAppliedUpdateId),
    );
}

describe('AppliedPatchState', () => {
    test('a cancelled update restores the state from before it', () => {
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);
        submit(state, 'b', 'song-b', 2, 1);

        state.resolve(2);

        expect(state.baseline(2)).toEqual({
            patchGraph: graph('a'),
            sourceId: 'song-a',
        });
    });

    test('an update cancelled after a newer submission is not rolled back', () => {
        // Update 1 applied before the cancel reached the audio thread, so the
        // reported cancelled id belongs to no update this state tracks.
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);
        submit(state, 'b', 'song-b', 2, 1);

        state.resolve(1);
        state.resolve(0);

        expect(state.baseline(2).sourceId).toBe('song-b');
    });

    test('a rollback happens once', () => {
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);
        submit(state, 'b', 'song-b', 2, 1);
        state.resolve(2);
        submit(state, 'c', 'song-a', 3, 1);

        // The meter still reports update 2 as the last cancelled one.
        state.resolve(2);

        expect(state.baseline(3).patchGraph).toEqual(graph('c'));
    });

    test('cancelling the first update restores the empty state', () => {
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);

        state.resolve(1);

        expect(state.baseline(1)).toEqual({ patchGraph: null, sourceId: null });
    });

    test('an applied update is the baseline for the next one', () => {
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);
        submit(state, 'b', 'song-a', 2, 1);

        expect(state.baseline(2).patchGraph).toEqual(graph('b'));
    });

    test('a queued update the engine has not applied is not the baseline', () => {
        // Update 2 is still queued; the next submission discards it and
        // applies against the patch playing now, update 1's.
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);
        submit(state, 'b', 'song-a', 2, 1);

        expect(state.baseline(1).patchGraph).toEqual(graph('a'));
    });

    test('a superseding update keeps the playing patch as its baseline', () => {
        // Update 3 discards queued update 2, so a submission made before
        // either applies still compares against update 1's patch.
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);
        submit(state, 'b', 'song-a', 2, 1);
        submit(state, 'b', 'song-a', 3, 1);

        expect(state.baseline(1).patchGraph).toEqual(graph('a'));
        expect(state.baseline(3).patchGraph).toEqual(graph('b'));
    });

    test('cancelling a superseding update restores the playing patch', () => {
        const state = new AppliedPatchState();
        submit(state, 'a', 'song-a', 1, 0);
        submit(state, 'b', 'song-a', 2, 1);
        submit(state, 'c', 'song-a', 3, 1);

        state.resolve(3);

        expect(state.baseline(1).patchGraph).toEqual(graph('a'));
    });
});
