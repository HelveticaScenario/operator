import type { PatchGraph } from '@modular/core';

type Applied = { patchGraph: PatchGraph | null; sourceId: string | null };

/**
 * The patch graph and source buffer that reconciliation and buffer-switch
 * detection compare the next update against.
 *
 * An update counts as applied once the engine accepts it, even while queued.
 * A cancel only takes effect if the audio thread discards the update before
 * its trigger fires, so `resolve` checks the reported cancelled id before
 * each submission and restores the prior state when it matches.
 */
export class AppliedPatchState {
    private current: Applied = { patchGraph: null, sourceId: null };
    private beforeLatest: (Applied & { updateId: number }) | null = null;

    get patchGraph(): PatchGraph | null {
        return this.current.patchGraph;
    }

    get sourceId(): string | null {
        return this.current.sourceId;
    }

    /** Restore the pre-update state if the latest update was cancelled. */
    resolve(lastCancelledUpdateId: number): void {
        if (this.beforeLatest?.updateId === lastCancelledUpdateId) {
            const { patchGraph, sourceId } = this.beforeLatest;
            this.current = { patchGraph, sourceId };
            this.beforeLatest = null;
        }
    }

    record(
        patchGraph: PatchGraph,
        sourceId: string | null,
        updateId: number,
    ): void {
        this.beforeLatest = { ...this.current, updateId };
        this.current = { patchGraph, sourceId };
    }
}
