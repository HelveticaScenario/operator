/**
 * Live spectrum for the EQ panel, computed in the renderer from a headless
 * full-rate scope. The scope ring holds only 1024 samples, too short for
 * useful low-end resolution, so successive polls are stitched into a longer
 * history by the scope's write position. A poll gap long enough to overrun
 * the ring breaks continuity, so the history restarts and the spectrum holds
 * until a full window of continuous samples has arrived again.
 */

export const SPECTRUM_FFT_SIZE = 4096;
/** Display floor and ceiling (dB re 5 V, after tilt). */
export const SPECTRUM_FLOOR_DB = -96;
export const SPECTRUM_CEIL_DB = 0;
/** Tilt raising highs relative to 1 kHz, so typical program material,
 *  whose energy falls with frequency, reads roughly flat. */
const TILT_DB_PER_OCT = 4.5;
/** Release of the displayed level; rises are immediate. */
const RELEASE_DB_PER_S = 60;

/** In-place iterative radix-2 FFT over separate real/imag arrays. */
export function fft(re: Float64Array, im: Float64Array): void {
    const n = re.length;
    for (let i = 1, j = 0; i < n; i++) {
        let bit = n >> 1;
        for (; j & bit; bit >>= 1) {
            j ^= bit;
        }
        j ^= bit;
        if (i < j) {
            [re[i], re[j]] = [re[j], re[i]];
            [im[i], im[j]] = [im[j], im[i]];
        }
    }
    for (let len = 2; len <= n; len <<= 1) {
        const ang = (-2 * Math.PI) / len;
        const wRe = Math.cos(ang);
        const wIm = Math.sin(ang);
        for (let i = 0; i < n; i += len) {
            let curRe = 1;
            let curIm = 0;
            for (let k = 0; k < len / 2; k++) {
                const aRe = re[i + k];
                const aIm = im[i + k];
                const bRe =
                    re[i + k + len / 2] * curRe - im[i + k + len / 2] * curIm;
                const bIm =
                    re[i + k + len / 2] * curIm + im[i + k + len / 2] * curRe;
                re[i + k] = aRe + bRe;
                im[i + k] = aIm + bIm;
                re[i + k + len / 2] = aRe - bRe;
                im[i + k + len / 2] = aIm - bIm;
                const nextRe = curRe * wRe - curIm * wIm;
                curIm = curRe * wIm + curIm * wRe;
                curRe = nextRe;
            }
        }
    }
}

export class SpectrumAnalyzer {
    private readonly history = new Float32Array(SPECTRUM_FFT_SIZE);
    /** Next write index into `history`. */
    private head = 0;
    private lastOffset = -1;
    private lastMs = 0;
    /** Samples at the end of `history` received without a break. */
    private continuous = 0;
    private readonly window = new Float64Array(SPECTRUM_FFT_SIZE);
    private readonly windowSum: number;
    private readonly re = new Float64Array(SPECTRUM_FFT_SIZE);
    private readonly im = new Float64Array(SPECTRUM_FFT_SIZE);
    /** Displayed level per bin, dB after tilt. */
    readonly binsDb = new Float64Array(SPECTRUM_FFT_SIZE / 2).fill(
        SPECTRUM_FLOOR_DB,
    );
    private lastComputeMs = 0;
    sampleRate = 48000;

    constructor() {
        let sum = 0;
        for (let i = 0; i < SPECTRUM_FFT_SIZE; i++) {
            const w =
                0.5 -
                0.5 * Math.cos((2 * Math.PI * i) / (SPECTRUM_FFT_SIZE - 1));
            this.window[i] = w;
            sum += w;
        }
        this.windowSum = sum;
    }

    /**
     * Take one scope poll. `readOffset` is the ring index of the newest
     * sample; samples since the previous poll are appended to the history.
     * Returns whether new samples arrived.
     */
    push(ring: Float32Array, readOffset: number, nowMs: number): boolean {
        const cap = ring.length;
        if (cap === 0) {
            return false;
        }
        const elapsedSamples = ((nowMs - this.lastMs) / 1000) * this.sampleRate;
        let count: number;
        if (this.lastOffset < 0 || elapsedSamples >= cap) {
            count = cap;
            this.continuous = 0;
        } else {
            count = (readOffset - this.lastOffset + cap) % cap;
        }
        this.lastOffset = readOffset;
        this.lastMs = nowMs;
        if (count === 0) {
            return false;
        }
        const first = (readOffset - count + 1 + cap) % cap;
        for (let i = 0; i < count; i++) {
            this.history[this.head] = ring[(first + i) % cap];
            this.head = (this.head + 1) % SPECTRUM_FFT_SIZE;
        }
        this.continuous = Math.min(this.continuous + count, SPECTRUM_FFT_SIZE);
        return true;
    }

    /**
     * Recompute `binsDb` from the history, applying the release. Holds the
     * previous result until the history is one continuous window.
     */
    compute(nowMs: number): void {
        if (this.continuous < SPECTRUM_FFT_SIZE) {
            return;
        }
        const n = SPECTRUM_FFT_SIZE;
        for (let i = 0; i < n; i++) {
            this.re[i] = this.history[(this.head + i) % n] * this.window[i];
            this.im[i] = 0;
        }
        fft(this.re, this.im);
        const dt =
            this.lastComputeMs > 0 ? (nowMs - this.lastComputeMs) / 1000 : 0;
        this.lastComputeMs = nowMs;
        const fall = RELEASE_DB_PER_S * dt;
        const binHz = this.sampleRate / n;
        for (let k = 1; k < n / 2; k++) {
            // Single-sided amplitude, so a full-scale (5 V) sine reads 0 dB.
            const amp =
                (2 * Math.hypot(this.re[k], this.im[k])) / this.windowSum;
            const db =
                20 * Math.log10(Math.max(amp, 1e-9) / 5) +
                TILT_DB_PER_OCT * Math.log2((k * binHz) / 1000);
            const prev = this.binsDb[k];
            this.binsDb[k] = db >= prev ? db : Math.max(db, prev - fall);
        }
    }

    /** Drop to the floor, e.g. when audio stops. */
    reset(): void {
        this.history.fill(0);
        this.binsDb.fill(SPECTRUM_FLOOR_DB);
        this.lastOffset = -1;
        this.lastComputeMs = 0;
        this.continuous = 0;
    }

    /**
     * Level at each display frequency: the loudest bin within each pixel's
     * span where bins are dense, interpolated between bins where sparse.
     */
    sample(freqsHz: ArrayLike<number>, out: Float64Array): void {
        const binHz = this.sampleRate / SPECTRUM_FFT_SIZE;
        const last = SPECTRUM_FFT_SIZE / 2 - 1;
        for (let i = 0; i < freqsHz.length; i++) {
            const lo =
                i > 0 ? Math.sqrt(freqsHz[i - 1] * freqsHz[i]) : freqsHz[i];
            const hi =
                i < freqsHz.length - 1
                    ? Math.sqrt(freqsHz[i] * freqsHz[i + 1])
                    : freqsHz[i];
            const kLo = Math.ceil(lo / binHz);
            const kHi = Math.floor(hi / binHz);
            if (kHi >= kLo && kLo >= 1) {
                let max = SPECTRUM_FLOOR_DB;
                for (let k = kLo; k <= Math.min(kHi, last); k++) {
                    max = Math.max(max, this.binsDb[k]);
                }
                out[i] = max;
            } else {
                const pos = Math.min(Math.max(freqsHz[i] / binHz, 1), last);
                const k0 = Math.floor(pos);
                const k1 = Math.min(k0 + 1, last);
                const t = pos - k0;
                out[i] = this.binsDb[k0] * (1 - t) + this.binsDb[k1] * t;
            }
        }
    }
}
