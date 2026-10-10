import type { PatchGraph } from '@modular/core';

type Applied = { patchGraph: PatchGraph | null; sourceId: string | null };
type Submission = Applied & { updateId: number };

const EMPTY: Applied = { patchGraph: null, sourceId: null };

/** Submissions older than this can no longer be the playing patch. */
const MAX_SUBMISSIONS = 16;

/**
 * The patches submitted to the engine, which reconciliation and buffer-switch
 * detection compare the next update against.
 *
 * An update counts as submitted once the engine accepts it, even while queued.
 * The engine discards a queued update when another arrives and applies the
 * newer one against the patch that is playing, so what a new update replaces
 * is the latest submission the engine has applied, found from the applied id
 * the engine reports. A cancel only takes effect if the audio thread discards
 * the update before its trigger fires, so `resolve` drops the submission the
 * engine reports as cancelled.
 */
export class AppliedPatchState {
    private submissions: Submission[] = [];

    /** The state playing once the engine has applied updates up to this id. */
    baseline(lastAppliedUpdateId: number): Applied {
        for (let i = this.submissions.length - 1; i >= 0; i--) {
            if (this.submissions[i].updateId <= lastAppliedUpdateId) {
                const { patchGraph, sourceId } = this.submissions[i];
                return { patchGraph, sourceId };
            }
        }
        return EMPTY;
    }

    /** Forget a submission the engine reports as cancelled. */
    resolve(lastCancelledUpdateId: number): void {
        this.submissions = this.submissions.filter(
            (s) => s.updateId !== lastCancelledUpdateId,
        );
    }

    record(
        patchGraph: PatchGraph,
        sourceId: string | null,
        updateId: number,
    ): void {
        this.submissions.push({ patchGraph, sourceId, updateId });
        if (this.submissions.length > MAX_SUBMISSIONS) {
            this.submissions.shift();
        }
    }

    /** The engine holds no patch any more. */
    clear(): void {
        this.submissions = [];
    }
}
