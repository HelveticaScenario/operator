import { describe, expect, it } from 'vitest';
import { TapStream } from './TapStream';

const RATE = 48_000;

/** A 1 Hz ramp-up/ramp-down triangle in 0..1, at sample `i`. */
const lfo = (i: number) => {
    const phase = (i / RATE) % 1;
    return phase < 0.5 ? phase * 2 : 2 - phase * 2;
};

/** Feeds `stream` like the engine does: whole audio callbacks at a time. */
function runEngineLike(
    stream: TapStream,
    seconds: number,
    options: { block: number; pollMs: number; frameMs: number },
): number[] {
    const values: number[] = [];
    let produced = 0;
    let pushed = 0;
    let nextPoll = 0;
    for (let t = 0; t <= seconds * 1000; t += options.frameMs) {
        // The engine has produced whole blocks up to this time.
        produced =
            Math.floor((t / 1000) * RATE + options.block) -
            (Math.floor((t / 1000) * RATE + options.block) % options.block);
        if (t >= nextPoll) {
            const chunk = new Float32Array(produced - pushed);
            for (let i = 0; i < chunk.length; i++) chunk[i] = lfo(pushed + i);
            stream.push(chunk, RATE);
            pushed = produced;
            nextPoll = t + options.pollMs;
        }
        stream.advance(t);
        values.push(stream.value());
    }
    return values;
}

const stepSizes = (values: number[]) =>
    values.slice(1).map((v, i) => Math.abs(v - values[i]));

describe('TapStream', () => {
    it('reads 0 before any data arrives', () => {
        const stream = new TapStream();
        stream.advance(0);
        expect(stream.value()).toBe(0);
    });

    it('moves smoothly although samples arrive in audio-callback bursts', () => {
        const stream = new TapStream();
        const values = runEngineLike(stream, 2, {
            block: 512,
            frameMs: 1000 / 120,
            pollMs: 16,
        });
        // A 1 Hz triangle between 0 and 1 changes by 2 per second, so a
        // 120 Hz frame should change it by 2 / 120 = 0.0167 and never jump.
        const steps = stepSizes(values.slice(30));
        expect(Math.max(...steps)).toBeLessThan(0.03);
        const mean = steps.reduce((a, b) => a + b, 0) / steps.length;
        expect(mean).toBeGreaterThan(0.012);
        expect(mean).toBeLessThan(0.02);
    });

    it('does not repeat a value on consecutive frames', () => {
        const stream = new TapStream();
        const values = runEngineLike(stream, 1, {
            block: 1024,
            frameMs: 1000 / 120,
            pollMs: 16,
        });
        const repeats = stepSizes(values.slice(30)).filter((s) => s === 0);
        expect(repeats).toHaveLength(0);
    });

    it('stays within the target latency of the newest sample', () => {
        const stream = new TapStream();
        const values = runEngineLike(stream, 3, {
            block: 512,
            frameMs: 1000 / 60,
            pollMs: 16,
        });
        // Compare against the true signal at the same time, allowing for the
        // fixed latency the stream deliberately adds (about 50 ms of a 1 Hz LFO).
        const last = values[values.length - 1];
        const t = 3;
        const expected = lfo(Math.round((t - 0.05) * RATE));
        expect(Math.abs(last - expected)).toBeLessThan(0.15);
    });

    it('recovers after a stall instead of racing to catch up', () => {
        const stream = new TapStream();
        stream.push(
            Float32Array.from({ length: 48_000 }, (_, i) => i),
            RATE,
        );
        stream.advance(0);
        stream.advance(10);
        stream.advance(5000);
        const afterStall = stream.value();
        expect(afterStall).toBeGreaterThan(47_000 - 2600);
        expect(afterStall).toBeLessThanOrEqual(47_999);
    });

    it('returns the samples leading up to the cursor, oldest first', () => {
        const stream = new TapStream();
        stream.push(
            Float32Array.from({ length: 4800 }, (_, i) => i),
            RATE,
        );
        stream.advance(0);
        const out = new Float32Array(4);
        stream.recent(out);
        expect(out[3] - out[2]).toBe(1);
        expect(out[1] - out[0]).toBe(1);
    });

    it('pads with silence before the first sample', () => {
        const stream = new TapStream();
        stream.push(Float32Array.from([5, 6]), RATE);
        stream.advance(0);
        const out = new Float32Array(6);
        stream.recent(out);
        expect(out[0]).toBe(0);
    });
});
