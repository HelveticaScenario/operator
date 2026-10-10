import type { PatchGraph } from '@modular/core';
import { describe, expect, test } from 'vitest';
import { AppliedPatchState } from '../appliedPatchState';

const graph = (name: string) => ({ name }) as unknown as PatchGraph;

describe('AppliedPatchState', () => {
    test('nothing submitted means an empty baseline', () => {
        const state = new AppliedPatchState();

        expect(state.baseline(0)).toEqual({ patchGraph: null, sourceId: null });
    });

    test('an applied update is the baseline for the next one', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-a', 2);

        expect(state.baseline(2).patchGraph).toEqual(graph('b'));
    });

    test('a queued update the engine has not applied is not the baseline', () => {
        // Update 2 is still queued; the next submission discards it and
        // applies against the patch playing now, update 1's.
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-a', 2);

        expect(state.baseline(1).patchGraph).toEqual(graph('a'));
    });

    test('a discarded update never becomes the baseline', () => {
        // Update 3 discards queued update 2 and applies immediately.
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-a', 2);
        state.record(graph('c'), 'song-a', 3);

        expect(state.baseline(1).patchGraph).toEqual(graph('a'));
        expect(state.baseline(3).patchGraph).toEqual(graph('c'));
    });

    test('an update applied while a later one is in flight is the baseline', () => {
        // Update 2 fired after update 3's baseline was read; the engine now
        // reports 2, so the next reconciliation must compare against it.
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-a', 2);
        state.record(graph('c'), 'song-a', 3);

        expect(state.baseline(2).patchGraph).toEqual(graph('b'));
    });

    test('a cancelled update is forgotten', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-b', 2);

        state.resolve(2);

        expect(state.baseline(2)).toEqual({
            patchGraph: graph('a'),
            sourceId: 'song-a',
        });
    });

    test('cancelling a superseding update restores the playing patch', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-a', 2);
        state.record(graph('c'), 'song-a', 3);

        state.resolve(3);

        expect(state.baseline(1).patchGraph).toEqual(graph('a'));
    });

    test('a cancelled id the meter keeps reporting changes nothing later', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);
        state.record(graph('b'), 'song-b', 2);
        state.resolve(2);
        state.record(graph('c'), 'song-a', 3);

        // The meter still reports update 2 as the last cancelled one.
        state.resolve(2);

        expect(state.baseline(3).patchGraph).toEqual(graph('c'));
    });

    test('cancelling the first update restores the empty state', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);

        state.resolve(1);

        expect(state.baseline(1)).toEqual({ patchGraph: null, sourceId: null });
    });

    test('clearing forgets every submission', () => {
        const state = new AppliedPatchState();
        state.record(graph('a'), 'song-a', 1);

        state.clear();

        expect(state.baseline(1)).toEqual({ patchGraph: null, sourceId: null });
    });
});
