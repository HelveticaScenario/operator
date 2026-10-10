import type { PatchGraph } from '@modular/core';

type Applied = { patchGraph: PatchGraph | null; sourceId: string | null };

/**
 * The patch graph and source buffer that reconciliation and buffer-switch
 * detection compare the next update against.
 *
 * An update counts as submitted once the engine accepts it, even while queued.
 * The engine discards a queued update when another arrives and applies the
 * newer one immediately against the patch that is actually playing, so the
 * baseline for a new update is the playing patch, not the discarded one.
 * A cancel only takes effect if the audio thread discards the update before
 * its trigger fires, so `resolve` checks the reported cancelled id before
 * each submission and restores the prior state when it matches.
 */
export class AppliedPatchState {
    private current: Applied = { patchGraph: null, sourceId: null };
    private beforeLatest: (Applied & { updateId: number }) | null = null;

    /**
     * The state the next update replaces: the latest submission once the
     * engine has applied it, otherwise the state playing before it.
     */
    baseline(lastAppliedUpdateId: number): Applied {
        if (
            this.beforeLatest &&
            this.beforeLatest.updateId > lastAppliedUpdateId
        ) {
            const { patchGraph, sourceId } = this.beforeLatest;
            return { patchGraph, sourceId };
        }
        return this.current;
    }

    /** Restore the pre-update state if the latest update was cancelled. */
    resolve(lastCancelledUpdateId: number): void {
        if (this.beforeLatest?.updateId === lastCancelledUpdateId) {
            const { patchGraph, sourceId } = this.beforeLatest;
            this.current = { patchGraph, sourceId };
            this.beforeLatest = null;
        }
    }

    /** Record an accepted update that replaces `baseline`. */
    record(
        patchGraph: PatchGraph,
        sourceId: string | null,
        updateId: number,
        baseline: Applied,
    ): void {
        this.beforeLatest = { ...baseline, updateId };
        this.current = { patchGraph, sourceId };
    }
}
