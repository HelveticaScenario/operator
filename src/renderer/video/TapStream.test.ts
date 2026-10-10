import { describe, expect, it } from 'vitest';
import { TapStream } from './TapStream';

interface Setup {
    sampleRate: number;
    /** Samples per audio callback; the engine finishes samples this many at a time. */
    block: number;
    /** Display refresh rate; the renderer asks for new samples once per frame. */
    frameHz: number;
    /** Time for a request to reach the engine and the answer to come back, in ms. */
    roundTripMs?: number;
    /** Largest random extra delay on top of the round trip, in ms. */
    jitterMs?: number;
}

/**
 * Plays the engine and the renderer against each other for `seconds`. The
 * engine's signal is a ramp (sample i has value i), so the cursor's value is
 * its position in samples. Returns, for each frame, the cursor's value and how
 * far it trails real time, in seconds.
 */
function simulate(setup: Setup, seconds: number) {
    const { sampleRate, block, frameHz } = setup;
    const roundTrip = setup.roundTripMs ?? 1;
    const jitter = setup.jitterMs ?? 0.5;
    const frameMs = 1000 / frameHz;
    const stream = new TapStream();
    const arrivals: { at: number; from: number; to: number }[] = [];
    let pulled = 0;
    let seed = 12345;
    const random = () => {
        seed = (seed * 1664525 + 1013904223) % 4294967296;
        return seed / 4294967296;
    };

    const frames: { value: number; lagSeconds: number }[] = [];
    for (let t = 0; t <= seconds * 1000; t += frameMs) {
        // The renderer asks at the start of the frame; the engine has finished
        // whole blocks up to that moment, and the answer lands a round trip later.
        const finished = Math.floor(((t / 1000) * sampleRate) / block) * block;
        if (finished > pulled) {
            arrivals.push({
                at: t + roundTrip + random() * jitter,
                from: pulled,
                to: finished,
            });
            pulled = finished;
        }
        while (arrivals.length > 0 && arrivals[0].at <= t) {
            const { at, from, to } = arrivals.shift()!;
            const chunk = new Float32Array(to - from);
            for (let i = 0; i < chunk.length; i++) chunk[i] = from + i;
            stream.push(chunk, sampleRate, at);
        }
        stream.advance(t);
        const value = stream.value();
        frames.push({
            value,
            lagSeconds:
                (t / 1000) * sampleRate - value > 0
                    ? ((t / 1000) * sampleRate - value) / sampleRate
                    : 0,
        });
    }
    return frames;
}

const analyse = (setup: Setup, seconds = 6, warmupSeconds = 3) => {
    const frames = simulate(setup, seconds);
    const body = frames.slice(Math.floor(warmupSeconds * setup.frameHz));
    const steps = body.slice(1).map((f, i) => f.value - body[i].value);
    const ideal = setup.sampleRate / setup.frameHz;
    return {
        stalls: steps.filter((s) => s <= 0).length,
        smallest: Math.min(...steps) / ideal,
        largest: Math.max(...steps) / ideal,
        lagMs: Math.max(...body.map((f) => f.lagSeconds)) * 1000,
    };
};

describe('TapStream', () => {
    it('reads 0 before any data arrives', () => {
        const stream = new TapStream();
        stream.advance(0);
        expect(stream.value()).toBe(0);
    });

    it.each([
        {
            name: '60 Hz, 512 @ 44.1 kHz',
            sampleRate: 44_100,
            block: 512,
            frameHz: 60,
        },
        {
            name: '120 Hz, 512 @ 44.1 kHz',
            sampleRate: 44_100,
            block: 512,
            frameHz: 120,
        },
        {
            name: '144 Hz, 128 @ 48 kHz',
            sampleRate: 48_000,
            block: 128,
            frameHz: 144,
        },
        {
            name: '60 Hz, 1024 @ 96 kHz',
            sampleRate: 96_000,
            block: 1024,
            frameHz: 60,
        },
        {
            name: '75 Hz, 256 @ 48 kHz',
            sampleRate: 48_000,
            block: 256,
            frameHz: 75,
        },
        {
            name: '30 Hz, 512 @ 44.1 kHz',
            sampleRate: 44_100,
            block: 512,
            frameHz: 30,
        },
    ])('moves smoothly with no stalls: $name', (setup) => {
        const result = analyse(setup);
        expect(result.stalls).toBe(0);
        // Every frame moves by about one frame's worth of samples; a stall
        // or a jump would show as a step far from 1.
        expect(result.smallest).toBeGreaterThan(0.8);
        expect(result.largest).toBeLessThan(1.25);
    });

    it.each([
        {
            name: '120 Hz, 512 @ 44.1 kHz',
            sampleRate: 44_100,
            block: 512,
            frameHz: 120,
            limitMs: 35,
        },
        {
            name: '60 Hz, 512 @ 44.1 kHz',
            sampleRate: 44_100,
            block: 512,
            frameHz: 60,
            limitMs: 40,
        },
        {
            name: '144 Hz, 128 @ 48 kHz',
            sampleRate: 48_000,
            block: 128,
            frameHz: 144,
            limitMs: 20,
        },
        {
            name: '75 Hz, 256 @ 48 kHz',
            sampleRate: 48_000,
            block: 256,
            frameHz: 75,
            limitMs: 30,
        },
    ])(
        'trails real time by less than the old fixed 50 ms: $name',
        ({ limitMs, ...setup }) => {
            expect(analyse(setup).lagMs).toBeLessThan(limitMs);
        },
    );

    it('trails less with a smaller audio buffer and a faster display', () => {
        const coarse = analyse({
            sampleRate: 48_000,
            block: 2048,
            frameHz: 60,
        });
        const fine = analyse({ sampleRate: 48_000, block: 128, frameHz: 144 });
        expect(fine.lagMs).toBeLessThan(coarse.lagMs / 2);
        // Even a very coarse buffer, which hands over data in 43 ms bursts, stays smooth.
        expect(coarse.stalls).toBe(0);
    });

    it('stays smooth under transport jitter', () => {
        const result = analyse({
            sampleRate: 48_000,
            block: 512,
            frameHz: 120,
            roundTripMs: 2,
            jitterMs: 6,
        });
        expect(result.stalls).toBe(0);
        expect(result.largest).toBeLessThan(1.4);
    });

    it('widens its trail when a slower batch arrives, then settles back', () => {
        const stream = new TapStream();
        const rate = 48_000;
        let written = 0;
        const send = (at: number, samples: number) => {
            stream.push(
                Float32Array.from({ length: samples }, (_, i) => written + i),
                rate,
                at,
            );
            written += samples;
        };
        // How many samples the cursor trails the newest one by.
        const lagAt = () => written - stream.value();
        // Steady 8 ms batches...
        for (let t = 0; t <= 2000; t += 8) {
            send(t, 384);
            stream.advance(t);
        }
        const steadyLag = lagAt();
        // ...then one 60 ms gap...
        let t = 2008;
        stream.advance(t);
        t += 60;
        send(t, 60 * 48);
        stream.advance(t);
        // ...and the trail grows so the next such gap cannot run it dry.
        for (let i = 0; i < 100; i++) {
            t += 8;
            send(t, 384);
            stream.advance(t);
        }
        expect(lagAt()).toBeGreaterThan(steadyLag);
        // Once the gap has aged out of the window, the trail shrinks again.
        for (let i = 0; i < 700; i++) {
            t += 8;
            send(t, 384);
            stream.advance(t);
        }
        expect(lagAt()).toBeLessThan(steadyLag * 1.5);
    });

    it('starts over when the sample rate changes', () => {
        const stream = new TapStream();
        stream.push(
            Float32Array.from({ length: 4410 }, (_, i) => i),
            44_100,
            0,
        );
        stream.advance(0);
        stream.advance(10);
        expect(stream.value()).toBeGreaterThan(0);
        stream.push(
            Float32Array.from({ length: 4800 }, () => 7),
            48_000,
            20,
        );
        stream.advance(20);
        stream.advance(30);
        // Nothing from the old timeline is left to read.
        expect(stream.value()).toBe(7);
    });

    it('recovers after a stall instead of racing to catch up', () => {
        const stream = new TapStream();
        stream.push(
            Float32Array.from({ length: 48_000 }, (_, i) => i),
            48_000,
            0,
        );
        stream.advance(0);
        stream.advance(10);
        stream.advance(5000);
        const afterStall = stream.value();
        expect(afterStall).toBeGreaterThan(47_000 - 4000);
        expect(afterStall).toBeLessThanOrEqual(47_999);
    });

    it('returns the samples leading up to the cursor, oldest first', () => {
        const stream = new TapStream();
        stream.push(
            Float32Array.from({ length: 4800 }, (_, i) => i),
            48_000,
            0,
        );
        stream.advance(0);
        const out = new Float32Array(4);
        stream.recent(out);
        expect(out[3] - out[2]).toBe(1);
        expect(out[1] - out[0]).toBe(1);
    });

    it('pads with silence before the first sample', () => {
        const stream = new TapStream();
        stream.push(Float32Array.from([5, 6]), 48_000, 0);
        stream.advance(0);
        const out = new Float32Array(6);
        stream.recent(out);
        expect(out[0]).toBe(0);
    });

    it('reports whether any samples have arrived', () => {
        const stream = new TapStream();
        expect(stream.hasData).toBe(false);
        stream.push([], 48000, 0);
        expect(stream.hasData).toBe(false);
        stream.push([0.5], 48000, 0);
        expect(stream.hasData).toBe(true);
    });
});
