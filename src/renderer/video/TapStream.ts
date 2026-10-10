/** Samples the ring keeps; a power of two so indexing is a mask. */
const RING = 16384;
/** Shortest and longest the cursor will trail the newest sample, in seconds. */
const MIN_LATENCY = 0.004;
const MAX_LATENCY = 0.25;
/** The trail before any gap between batches has been seen, in seconds. */
const INITIAL_LATENCY = 0.03;
/** How long a gap between batches counts toward the trail, in milliseconds. */
const GAP_WINDOW_MS = 3000;
/** The trail is the widest recent gap, widened by this factor and padding. */
const GAP_MARGIN = 1.2;
const GAP_PADDING = 0.002;
/** Largest speed-up or slow-down the cursor applies to reach its trail. */
const MAX_RATE_ADJUST = 0.1;
/** A gap this long between reads is a stall; the cursor jumps instead of racing. */
const STALL_SECONDS = 0.25;

/**
 * Plays an audio-rate signal back against the display clock. The engine hands
 * samples over in bursts, one audio callback at a time, and the renderer picks
 * them up once per display frame, so "the newest sample" moves in jumps. The
 * cursor here advances smoothly at the sample rate instead, trailing the newest
 * sample just far enough that it never runs out before the next batch.
 *
 * That trail is set from what actually arrives: the cursor consumes audio while
 * it waits, so it must trail by at least the longest gap between batches. The
 * gap already contains the audio buffer size, the display's refresh rate and
 * any transport jitter, so nothing about them is assumed.
 */
export class TapStream {
    private readonly ring = new Float32Array(RING);
    private written = 0;
    /** Fractional index of the cursor among all samples ever pushed; -1 before data. */
    private cursor = -1;
    private lastMs = 0;
    private rate = 0;
    private lastArrivalMs = -1;
    private gaps: { at: number; gap: number }[] = [];

    /** Whether any samples have arrived. */
    get hasData(): boolean {
        return this.written > 0;
    }

    /** Appends samples oldest first, which arrived at `nowMs`. */
    push(samples: ArrayLike<number>, sampleRate: number, nowMs: number): void {
        if (sampleRate !== this.rate) this.reset(sampleRate);
        if (samples.length === 0) return;
        for (let i = 0; i < samples.length; i++) {
            this.ring[(this.written + i) & (RING - 1)] = samples[i];
        }
        this.written += samples.length;
        if (this.lastArrivalMs >= 0) {
            this.gaps.push({ at: nowMs, gap: nowMs - this.lastArrivalMs });
        }
        this.lastArrivalMs = nowMs;
    }

    /** Moves the cursor to the wall-clock time `nowMs`. */
    advance(nowMs: number): void {
        if (this.written === 0) return;
        const target = this.latency(nowMs) * this.rate;
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

    /** How far the cursor should trail the newest sample, in seconds. */
    private latency(nowMs: number): number {
        const cutoff = nowMs - GAP_WINDOW_MS;
        while (this.gaps.length > 0 && this.gaps[0].at < cutoff) {
            this.gaps.shift();
        }
        if (this.gaps.length === 0) return INITIAL_LATENCY;
        let widest = 0;
        for (const { gap } of this.gaps) widest = Math.max(widest, gap);
        return Math.min(
            MAX_LATENCY,
            Math.max(MIN_LATENCY, (widest / 1000) * GAP_MARGIN + GAP_PADDING),
        );
    }

    /** Forgets everything: samples at another rate are on another timeline. */
    private reset(sampleRate: number): void {
        this.rate = sampleRate;
        this.written = 0;
        this.cursor = -1;
        this.lastArrivalMs = -1;
        this.gaps = [];
    }
}
