import { describe, expect, it } from 'vitest';

import { SpectrumAnalyzer, fft } from './eqSpectrum';

const SR = 48000;
const CAP = 1024;

/** Simulates the engine scope ring: writes advance, newest at readOffset. */
class FakeScope {
    ring = new Float32Array(CAP);
    write = 0;
    t = 0;
    feed(n: number, f: (t: number) => number) {
        for (let i = 0; i < n; i++) {
            this.ring[this.write] = f(this.t++);
            this.write = (this.write + 1) % CAP;
        }
    }
    get readOffset() {
        return (this.write - 1 + CAP) % CAP;
    }
}

describe('fft', () => {
    it('puts a pure bin-centered tone in its bin', () => {
        const n = 64;
        const re = new Float64Array(n);
        const im = new Float64Array(n);
        for (let i = 0; i < n; i++) {
            re[i] = Math.cos((2 * Math.PI * 5 * i) / n);
        }
        fft(re, im);
        expect(Math.hypot(re[5], im[5])).toBeCloseTo(n / 2, 6);
        expect(Math.hypot(re[6], im[6])).toBeLessThan(1e-9);
    });
});

describe('SpectrumAnalyzer', () => {
    it('stitches polls into a continuous history and finds the tone', () => {
        const an = new SpectrumAnalyzer();
        an.sampleRate = SR;
        const scope = new FakeScope();
        const tone = (t: number) => 5 * Math.sin((2 * Math.PI * 100 * t) / SR);
        let now = 1000;
        // 16 ms polls (768 samples) never overrun the 1024 ring.
        for (let i = 0; i < 12; i++) {
            scope.feed(768, tone);
            now += 16;
            an.push(scope.ring, scope.readOffset, now);
        }
        an.compute(now);
        const freqs = [50, 100, 400];
        const out = new Float64Array(3);
        an.sample(freqs, out);
        // A full-scale 100 Hz sine reads ~0 dB plus the tilt at 100 Hz
        // (log2(0.1) * 4.5 ≈ -15 dB); Hann scalloping stays under 1.5 dB.
        expect(out[1]).toBeGreaterThan(-15 - 2);
        expect(out[1]).toBeLessThan(-15 + 1);
        // A discontinuous stitch would smear energy across the spectrum.
        expect(out[2]).toBeLessThan(out[1] - 40);
        expect(out[0]).toBeLessThan(out[1] - 20);
    });

    it('restarts after a poll gap overruns the ring, never splicing across it', () => {
        const an = new SpectrumAnalyzer();
        an.sampleRate = SR;
        const scope = new FakeScope();
        const tone = (t: number) => 5 * Math.sin((2 * Math.PI * 100 * t) / SR);
        let now = 1000;
        for (let i = 0; i < 12; i++) {
            scope.feed(768, tone);
            now += 16;
            an.push(scope.ring, scope.readOffset, now);
        }
        an.compute(now);
        const before = Float64Array.from(an.binsDb);
        // A stall: the scope keeps writing, phase-jumped, past the ring size.
        scope.feed(5000, (t) => 5 * Math.sin((2 * Math.PI * 100 * t) / SR + 2));
        now += 200;
        an.push(scope.ring, scope.readOffset, now);
        an.compute(now);
        expect(Array.from(an.binsDb)).toEqual(Array.from(before));
        // Once a full continuous window arrives, it computes again.
        for (let i = 0; i < 6; i++) {
            scope.feed(768, tone);
            now += 16;
            an.push(scope.ring, scope.readOffset, now);
        }
        an.compute(now);
        const out = new Float64Array(2);
        an.sample([100, 400], out);
        expect(out[1]).toBeLessThan(out[0] - 40);
    });

    it('reports no new samples for an unchanged poll', () => {
        const an = new SpectrumAnalyzer();
        const scope = new FakeScope();
        scope.feed(100, () => 1);
        expect(an.push(scope.ring, scope.readOffset, 10)).toBe(true);
        expect(an.push(scope.ring, scope.readOffset, 11)).toBe(false);
    });
});
