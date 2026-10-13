/**
 * EQ section maths for the panel: param decoding, quantization, and the
 * magnitude response of one section. The response mirrors the engine's
 * trapezoidal SVF (`crates/modular_core/src/dsp/filters/eq/`) exactly —
 * including its param clamps and `effective_q` — so the drawn curve is the
 * filter that plays.
 */

import { parseSliderValue } from '../../shared/dsl/sliderUnits';

export type EqSectionType =
    | 'peak'
    | 'lowShelf'
    | 'highShelf'
    | 'lowpass'
    | 'highpass'
    | 'notch';

export const EQ_SECTION_TYPES: readonly EqSectionType[] = [
    'peak',
    'lowShelf',
    'highShelf',
    'lowpass',
    'highpass',
    'notch',
];

export const MAX_EQ_SECTIONS = 8;
/** Most per-channel section lists (the engine's port width). */
export const MAX_EQ_CHANNEL_LISTS = 64;
export const DEFAULT_Q = Math.SQRT1_2;
export const MIN_Q = 0.1;
export const MAX_Q = 20;
export const MAX_GAIN_DB = 24;
export const MIN_FREQ_HZ = 20;
export const MAX_FREQ_HZ = 20000;

/** Types whose `gain` shapes the response; the others ignore it. */
export function typeHasGain(type: EqSectionType): boolean {
    return type === 'peak' || type === 'lowShelf' || type === 'highShelf';
}

/**
 * A mono signal param's constant value in engine units (V/Oct for freq, raw
 * number otherwise), decoded the way the engine deserializes it: numbers,
 * `'440hz'`, and note names, with an array summed like the engine sums a
 * polyphonic value. Null when any part is signal-driven (a cable) or
 * unparsable — the panel cannot place or drag it.
 */
export function constantSignalValue(value: unknown): number | null {
    if (typeof value === 'number') {
        return Number.isFinite(value) ? value : null;
    }
    if (Array.isArray(value)) {
        let sum = 0;
        for (const v of value) {
            const c = constantSignalValue(v);
            if (c === null || Array.isArray(v)) {
                return null;
            }
            sum += c;
        }
        return value.length > 0 ? sum : null;
    }
    if (typeof value !== 'string') {
        return null;
    }
    try {
        return parseSliderValue(value).volts;
    } catch {
        return null;
    }
}

export function clamp(v: number, lo: number, hi: number): number {
    return Math.min(Math.max(v, lo), hi);
}

/** Frequency as the panel writes it: 0.1 Hz below 100 Hz, else 1 Hz. */
export function quantizeHz(hz: number): number {
    const f = clamp(hz, MIN_FREQ_HZ, MAX_FREQ_HZ);
    return f < 100 ? Math.round(f * 10) / 10 : Math.round(f);
}

/** Gain as the panel writes it: 0.1 dB steps. */
export function quantizeGain(db: number): number {
    const g = Math.round(clamp(db, -MAX_GAIN_DB, MAX_GAIN_DB) * 10) / 10;
    return Object.is(g, -0) ? 0 : g;
}

/** Q as the panel writes it: two significant digits. */
export function quantizeQ(q: number): number {
    return Number(clamp(q, MIN_Q, MAX_Q).toPrecision(2));
}

/** Short display text for a frequency, e.g. `57.3 Hz`, `1.25 kHz`. */
export function formatHz(hz: number): string {
    if (hz >= 1000) {
        const k = hz / 1000;
        return `${Number(k.toFixed(k >= 10 ? 1 : 2))} kHz`;
    }
    return `${Number(hz.toFixed(hz < 100 ? 1 : 0))} Hz`;
}

export function formatGain(db: number): string {
    return `${db > 0 ? '+' : ''}${db.toFixed(1)} dB`;
}

/** The Q a section's filter runs at (mirror of the engine's `effective_q`). */
export function effectiveQ(
    type: EqSectionType,
    q: number,
    gainDb: number,
    adaptive: boolean,
): number {
    if ((type === 'lowShelf' || type === 'highShelf') && q < DEFAULT_Q) {
        const a = Math.pow(10, gainDb / 40);
        const s = 2 * q * q;
        return 1 / Math.sqrt((a + 1 / a) * (1 / s - 1) + 2);
    }
    if (type === 'peak' && adaptive) {
        return Math.min(q * Math.pow(10, Math.abs(gainDb) / 40), MAX_Q);
    }
    return q;
}

export interface EqSectionValues {
    type: EqSectionType;
    freqHz: number;
    gainDb: number;
    q: number;
}

interface SvfCoeffs {
    g: number;
    k: number;
    m0: number;
    m1: number;
    m2: number;
}

function sectionCoeffs(
    sec: EqSectionValues,
    adaptive: boolean,
    sampleRate: number,
): SvfCoeffs {
    const freq = clamp(sec.freqHz, MIN_FREQ_HZ, sampleRate * 0.495);
    const gain = clamp(sec.gainDb, -MAX_GAIN_DB, MAX_GAIN_DB);
    const q = effectiveQ(sec.type, clamp(sec.q, MIN_Q, MAX_Q), gain, adaptive);
    const a = Math.pow(10, gain / 40);
    const g0 = Math.tan((Math.PI * freq) / sampleRate);
    switch (sec.type) {
        case 'peak': {
            const k = 1 / (q * a);
            return { g: g0, k, m0: 1, m1: k * (a * a - 1), m2: 0 };
        }
        case 'lowShelf': {
            const k = 1 / q;
            return {
                g: g0 / Math.sqrt(a),
                k,
                m0: 1,
                m1: k * (a - 1),
                m2: a * a - 1,
            };
        }
        case 'highShelf': {
            const k = 1 / q;
            return {
                g: g0 * Math.sqrt(a),
                k,
                m0: a * a,
                m1: k * (1 - a) * a,
                m2: 1 - a * a,
            };
        }
        case 'lowpass':
            return { g: g0, k: 1 / q, m0: 0, m1: 0, m2: 1 };
        case 'highpass': {
            const k = 1 / q;
            return { g: g0, k, m0: 1, m1: -k, m2: -1 };
        }
        case 'notch': {
            const k = 1 / q;
            return { g: g0, k, m0: 1, m1: -k, m2: 0 };
        }
    }
}

/**
 * A section's response in dB at each of `freqsHz`. Trapezoidal integration
 * is the bilinear transform, so at frequency f the SVF's integrator variable
 * is s = j·tan(πf/fs) and H = m0 + (m1·g·s + m2·g²) / (s² + k·g·s + g²).
 */
export function sectionResponseDb(
    sec: EqSectionValues,
    adaptive: boolean,
    sampleRate: number,
    freqsHz: ArrayLike<number>,
    out: Float64Array,
): void {
    const { g, k, m0, m1, m2 } = sectionCoeffs(sec, adaptive, sampleRate);
    const g2 = g * g;
    const nyquistGuard = sampleRate * 0.4999;
    for (let i = 0; i < freqsHz.length; i++) {
        const t = Math.tan(
            (Math.PI * Math.min(freqsHz[i], nyquistGuard)) / sampleRate,
        );
        const dRe = g2 - t * t;
        const dIm = k * g * t;
        const nRe = m0 * dRe + m2 * g2;
        const nIm = m0 * dIm + m1 * g * t;
        const mag2 = (nRe * nRe + nIm * nIm) / (dRe * dRe + dIm * dIm);
        out[i] = 10 * Math.log10(Math.max(mag2, 1e-30));
    }
}
