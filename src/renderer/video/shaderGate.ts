/** What to do with a shader built for a patch update, given what the engine has done. */
export type GateVerdict = 'wait' | 'activate' | 'drop';

/**
 * A shader belongs to the audio patch it was compiled with: the audio taps it
 * reads are numbered by that patch. It goes live once the engine has applied
 * that update (or a later one), and is dropped if the engine discarded the
 * update, which leaves the previous patch and shader running.
 */
export function gateVerdict(
    updateId: number,
    applied: number,
    cancelled: number,
): GateVerdict {
    if (applied >= updateId) return 'activate';
    if (cancelled >= updateId) return 'drop';
    return 'wait';
}

/**
 * Holds a newly built shader back until the engine applies the patch update
 * it belongs to, as {@link gateVerdict} decides. `P` is the built shader; null
 * stands for a patch without video.
 */
export class ShaderGate<P extends { destroy(): void }> {
    /** Counts requests, so a build that finishes after a newer request is dropped. */
    private token = 0;
    /** The update id of the latest shader requested and not yet live. */
    private awaiting: number | null = null;
    /** A built shader waiting for the engine to apply its update. */
    private pending: { program: P | null; updateId: number } | null = null;
    /** The latest update ids the engine reported applying and discarding. */
    private applied = 0;
    private cancelled = 0;

    /** True from a shader's request until it goes live or is dropped. */
    get swapping(): boolean {
        return this.awaiting !== null;
    }

    /** True from the moment the engine applies the awaited update until its shader is live. */
    get holding(): boolean {
        return this.awaiting !== null && this.applied >= this.awaiting;
    }

    /** The built shader waiting to go live, if there is one. */
    get waiting(): P | null {
        return this.pending?.program ?? null;
    }

    /**
     * Starts waiting for the shader of `updateId`, dropping any shader built
     * for an earlier one. Returns the token to hand to {@link offer} or
     * {@link fail}.
     */
    request(updateId: number): number {
        this.pending?.program?.destroy();
        this.pending = null;
        this.awaiting = updateId;
        return ++this.token;
    }

    /** Stops waiting when the shader of request `token` failed to build. */
    fail(token: number): void {
        if (token === this.token) this.awaiting = null;
    }

    /**
     * Hands over the shader built for request `token`. Returns false, having
     * destroyed it, when a newer request has superseded it.
     */
    offer(token: number, program: P | null): boolean {
        if (token !== this.token || this.awaiting === null) {
            program?.destroy();
            return false;
        }
        this.pending = { program, updateId: this.awaiting };
        return true;
    }

    /** Records what the engine reports; returns whether its update numbering started over. */
    report(applied: number, cancelled: number): boolean {
        const restarted = applied < this.applied;
        this.applied = applied;
        this.cancelled = cancelled;
        return restarted;
    }

    /**
     * Releases the waiting shader if the engine has applied its update, or
     * drops it if the engine discarded the update. `force` releases it
     * regardless, for an engine whose update numbering has started over.
     * Returns the released shader, or undefined when none went live.
     */
    settle(force: boolean): { program: P | null } | undefined {
        const { pending } = this;
        if (pending === null) return undefined;
        const verdict = force
            ? 'activate'
            : gateVerdict(pending.updateId, this.applied, this.cancelled);
        if (verdict === 'wait') return undefined;
        this.pending = null;
        this.awaiting = null;
        if (verdict === 'drop') {
            pending.program?.destroy();
            return undefined;
        }
        return { program: pending.program };
    }

    /** Drops the waiting shader and any build still in flight. */
    dispose(): void {
        this.request(0);
        this.awaiting = null;
    }
}
