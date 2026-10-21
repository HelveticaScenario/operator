/**
 * The music's loudness as the party layer sees it: the engine's VU frames,
 * folded into one 0–1 level, a slow average of it, and a beat detector.
 * App.tsx pushes frames from its scope-polling loop while `wanted` is set.
 */

interface LevelFrame {
    rms: number[];
    peak: number[];
}

/** 5 V is full scale in the engine's voltage convention. */
const FULL_SCALE = 5;
/** How long after a beat the next one may land. */
const BEAT_REFRACTORY_MS = 170;

class PartyLevels {
    /** Set by the party layer while anything on screen dances. */
    wanted = false;
    /** Smoothed loudness, 0 to 1. */
    level = 0;
    /** Slow average of `level`, the floor a beat must rise above. */
    average = 0;
    /** 1 on a beat, decaying toward 0 between them. */
    pulse = 0;
    beats = 0;
    private lastBeatMs = 0;
    private lastPushMs = 0;
    private lastRaw = 0;
    private readonly beatListeners = new Set<(strength: number) => void>();

    push(frames: readonly LevelFrame[]): void {
        // An empty poll is a lost lock race, not silence.
        if (frames.length === 0) return;
        let raw = 0;
        for (const { peak, rms } of frames) {
            for (const v of rms) raw = Math.max(raw, v);
            for (const v of peak) raw = Math.max(raw, v * 0.6);
        }
        raw = Math.min(1, Math.sqrt(raw / FULL_SCALE));
        const now = performance.now();
        this.lastPushMs = now;
        this.level += (raw - this.level) * (raw > this.level ? 0.6 : 0.15);
        this.average += (raw - this.average) * 0.02;
        const rising = raw - this.lastRaw;
        this.lastRaw = raw;
        if (
            raw > 0.08 &&
            raw > this.average * 1.25 &&
            rising > 0.02 &&
            now - this.lastBeatMs > BEAT_REFRACTORY_MS
        ) {
            this.lastBeatMs = now;
            this.beats += 1;
            this.pulse = 1;
            const strength = Math.min(1, (raw - this.average) * 3 + 0.3);
            for (const listener of this.beatListeners) listener(strength);
        }
    }

    /** Advances the decays once per drawn frame. */
    tick(dtSeconds: number): void {
        this.pulse *= Math.exp(-dtSeconds * 6);
        // No frames for a while means the engine stopped.
        if (performance.now() - this.lastPushMs > 250) {
            this.level *= Math.exp(-dtSeconds * 4);
            this.average *= Math.exp(-dtSeconds * 1);
        }
    }

    onBeat(listener: (strength: number) => void): () => void {
        this.beatListeners.add(listener);
        return () => this.beatListeners.delete(listener);
    }
}

export const partyLevels = new PartyLevels();
