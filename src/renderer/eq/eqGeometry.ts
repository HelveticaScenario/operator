/**
 * EQ panel graph geometry: the log-frequency × dB mapping, where each
 * section's node and Q handles sit, and hit testing. All in CSS pixels.
 */
import {
    MAX_FREQ_HZ,
    MAX_GAIN_DB,
    MIN_FREQ_HZ,
    type EqSectionValues,
    typeHasGain,
} from './eqMath';
import type { EqSectionView } from './eqPanelModel';

export const NODE_RADIUS = 7;
const HIT_RADIUS = 11;

/** Per-section node colors, by section index. */
export const SECTION_COLORS = [
    '#f0a04b',
    '#5fb3f0',
    '#d47fd8',
    '#9fd94f',
    '#f06a6a',
    '#e8d26a',
    '#6ae0c8',
    '#b49cf5',
];

export interface EqGeometry {
    width: number;
    height: number;
    left: number;
    right: number;
    top: number;
    bottom: number;
    fMin: number;
    fMax: number;
    /** Frequency at each plot column, for curve and spectrum sampling. */
    freqs: Float64Array;
}

export function makeGeometry(
    width: number,
    height: number,
    sampleRate: number,
): EqGeometry {
    const left = 30;
    const right = width - 6;
    const top = 6;
    const bottom = height - 16;
    const fMin = MIN_FREQ_HZ;
    const fMax = Math.min(MAX_FREQ_HZ, sampleRate * 0.495);
    const cols = Math.max(2, Math.ceil(right - left) + 1);
    const freqs = new Float64Array(cols);
    for (let i = 0; i < cols; i++) {
        freqs[i] = fMin * Math.pow(fMax / fMin, i / (cols - 1));
    }
    return { bottom, fMax, fMin, freqs, height, left, right, top, width };
}

export function freqToX(g: EqGeometry, hz: number): number {
    return (
        g.left +
        (Math.log(hz / g.fMin) / Math.log(g.fMax / g.fMin)) * (g.right - g.left)
    );
}

export function xToFreq(g: EqGeometry, x: number): number {
    const t = (x - g.left) / (g.right - g.left);
    return g.fMin * Math.pow(g.fMax / g.fMin, t);
}

export function gainToY(g: EqGeometry, db: number): number {
    const mid = (g.top + g.bottom) / 2;
    return mid - (db / MAX_GAIN_DB) * ((g.bottom - g.top) / 2);
}

export function yToGain(g: EqGeometry, y: number): number {
    const mid = (g.top + g.bottom) / 2;
    return ((mid - y) / ((g.bottom - g.top) / 2)) * MAX_GAIN_DB;
}

/** Section values for the response, or null while any is unknown (a
 *  signal-driven param with no live value yet). */
export function knownValues(sec: EqSectionView): EqSectionValues | null {
    if (sec.freqHz === null || sec.gainDb === null || sec.q === null) {
        return null;
    }
    return {
        freqHz: sec.freqHz,
        gainDb: sec.gainDb,
        q: sec.q,
        type: sec.type,
    };
}

/**
 * Where a section's node sits: its freq, and its gain for types that use
 * gain (0 dB otherwise, or while gain is unknown). Null while freq is
 * unknown.
 */
export function nodePosition(
    g: EqGeometry,
    sec: Pick<EqSectionView, 'type' | 'freqHz' | 'gainDb'>,
): { x: number; y: number } | null {
    if (sec.freqHz === null) {
        return null;
    }
    const gain = typeHasGain(sec.type) && sec.gainDb !== null ? sec.gainDb : 0;
    return { x: freqToX(g, sec.freqHz), y: gainToY(g, gain) };
}

/** Bandwidth in octaves of a peak with this Q. */
export function qToOctaves(q: number): number {
    return (2 / Math.LN2) * Math.asinh(1 / (2 * q));
}

/** Q of a peak spanning `octaves`. */
export function octavesToQ(octaves: number): number {
    return 1 / (2 * Math.sinh((Math.LN2 * octaves) / 2));
}

/**
 * The selected section's Q handles: either side of its node, half its
 * bandwidth away, at the node's height. Null while freq or q is unknown.
 */
export function qHandles(
    g: EqGeometry,
    sec: EqSectionView,
): { x: number; y: number }[] | null {
    const node = nodePosition(g, sec);
    if (!node || sec.q === null || sec.freqHz === null) {
        return null;
    }
    const half = Math.pow(2, qToOctaves(sec.q) / 2);
    return [sec.freqHz / half, sec.freqHz * half].map((f) => ({
        x: freqToX(g, f),
        y: node.y,
    }));
}

/** True when (x, y) is on one of the section's Q handles. */
export function hitTestQHandle(
    g: EqGeometry,
    sec: EqSectionView,
    x: number,
    y: number,
): boolean {
    return (
        qHandles(g, sec)?.some(
            (h) => Math.abs(h.x - x) <= 6 && Math.abs(h.y - y) <= 10,
        ) ?? false
    );
}

/** Index of the node under (x, y), nearest first; selected wins ties. */
export function hitTestNode(
    g: EqGeometry,
    sections: EqSectionView[],
    x: number,
    y: number,
): number | null {
    let best: number | null = null;
    let bestDist = HIT_RADIUS;
    for (let i = sections.length - 1; i >= 0; i--) {
        const p = nodePosition(g, sections[i]);
        if (!p) {
            continue;
        }
        const d = Math.hypot(p.x - x, p.y - y);
        if (d < bestDist) {
            bestDist = d;
            best = i;
        }
    }
    return best;
}
