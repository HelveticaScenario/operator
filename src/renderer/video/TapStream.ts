/** Samples the ring keeps; a power of two so indexing is a mask. */
const RING = 16384;
/** How far behind the newest sample the cursor aims to run, in seconds. */
const TARGET_LATENCY = 0.05;
/** Largest speed-up or slow-down the cursor applies to hold that latency. */
const MAX_RATE_ADJUST = 0.05;
/** A gap this long between reads is a stall; the cursor jumps instead of racing. */
const STALL_SECONDS = 0.25;

/**
 * Plays an audio-rate signal back against the display clock. The engine hands
 * over samples in bursts, one audio callback at a time, so "the newest sample"
 * moves in jumps; the cursor here advances smoothly at the sample rate and
 * stays about {@link TARGET_LATENCY} behind the newest sample, nudging its
 * speed to absorb jitter, so every frame reads the signal exactly where it is
 * at that moment.
 */
export class TapStream {
    private readonly ring = new Float32Array(RING);
    private written = 0;
    /** Fractional index of the cursor among all samples ever pushed; -1 before data. */
    private cursor = -1;
    private lastMs = 0;
    private rate = 48_000;

    /** Appends samples oldest first. */
    push(samples: ArrayLike<number>, sampleRate: number): void {
        this.rate = sampleRate;
        for (let i = 0; i < samples.length; i++) {
            this.ring[(this.written + i) & (RING - 1)] = samples[i];
        }
        this.written += samples.length;
    }

    /** Moves the cursor to the wall-clock time `nowMs`. */
    advance(nowMs: number): void {
        if (this.written === 0) return;
        const target = this.rate * TARGET_LATENCY;
        const seconds = (nowMs - this.lastMs) / 1000;
        this.lastMs = nowMs;
        if (this.cursor < 0 || seconds > STALL_SECONDS || seconds < 0) {
            this.cursor = Math.max(0, this.written - target);
            return;
        }

        const backlog = this.written - this.cursor;
        const error = (backlog - target) / target;
        const speed =
            1 +
            Math.min(
                MAX_RATE_ADJUST,
                Math.max(-MAX_RATE_ADJUST, error * MAX_RATE_ADJUST),
            );
        this.cursor += seconds * this.rate * speed;

        const newest = this.written - 1;
        const oldest = Math.max(0, this.written - RING + 2);
        if (this.written - this.cursor > 4 * target) {
            this.cursor = Math.max(0, this.written - target);
        }
        this.cursor = Math.min(newest, Math.max(oldest, this.cursor));
    }

    /** The signal at the cursor, interpolated between samples; 0 before any data. */
    value(): number {
        if (this.cursor < 0) return 0;
        const i = Math.floor(this.cursor);
        const fraction = this.cursor - i;
        const a = this.ring[i & (RING - 1)];
        const b = this.ring[Math.min(i + 1, this.written - 1) & (RING - 1)];
        return a + (b - a) * fraction;
    }

    /**
     * Fills `into` with the samples leading up to the cursor, oldest first;
     * anything before the first sample reads as silence.
     */
    recent(into: Float32Array): void {
        const end = this.cursor < 0 ? -1 : Math.floor(this.cursor);
        for (let k = 0; k < into.length; k++) {
            const index = end - into.length + 1 + k;
            into[k] = index < 0 ? 0 : this.ring[index & (RING - 1)];
        }
    }
}
