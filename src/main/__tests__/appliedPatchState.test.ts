import type { PatchGraph } from '@modular/core';
import { describe, expect, test } from 'vitest';
import { AppliedPatchState } from '../appliedPatchState';

const graph = (name: string) => ({ name }) as unknown as PatchGraph;

describe('AppliedPatchState', () => {
    test('a cancelled update restores the state from before it', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-b', 2);

        state.resolve(2);

        expect(state.patchGraph).toEqual(graph('a'));
        expect(state.sourceId).toBe('song-a');
    });

    test('an update cancelled after a newer submission is not rolled back', () => {
        // Update 1 applied before the cancel reached the audio thread, so the
        // reported cancelled id belongs to no update this state tracks.
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-b', 2);

        state.resolve(1);
        state.resolve(0);

        expect(state.sourceId).toBe('song-b');
    });

    test('a rollback happens once', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-b', 2);
        state.resolve(2);
        state.record(graph('c'), 'song-a', 3);

        // The meter still reports update 2 as the last cancelled one.
        state.resolve(2);

        expect(state.patchGraph).toEqual(graph('c'));
    });

    test('cancelling the first update restores the empty state', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);

        state.resolve(1);

        expect(state.patchGraph).toBeNull();
        expect(state.sourceId).toBeNull();
    });
});
