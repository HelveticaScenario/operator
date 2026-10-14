/** Polls the engine's module states at a fixed rate while active. */

/** Poll interval for the live values of signal-driven section params. */
const LIVE_POLL_MS = 33;

export class EqLivePoller {
    private timer: number | null = null;
    private pending = false;

    constructor(
        private readonly getModuleStates: () => Promise<
            Record<string, unknown>
        >,
        private readonly onStates: (states: Record<string, unknown>) => void,
    ) {}

    setActive(active: boolean): void {
        if (active && this.timer === null) {
            this.timer = window.setInterval(() => this.poll(), LIVE_POLL_MS);
        } else if (!active && this.timer !== null) {
            window.clearInterval(this.timer);
            this.timer = null;
        }
    }

    /** One poll at a time; a slow reply skips ticks rather than piling up. */
    private poll(): void {
        if (this.pending) {
            return;
        }
        this.pending = true;
        void this.getModuleStates()
            .then(this.onStates)
            .catch(() => {})
            .finally(() => {
                this.pending = false;
            });
    }
}
