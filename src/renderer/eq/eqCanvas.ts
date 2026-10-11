/**
 * EQ panel graph: log-frequency × dB geometry, hit testing, and canvas
 * drawing of the grid, live spectrum, response curves, and section nodes.
 * All geometry is in CSS pixels; drawing scales by devicePixelRatio.
 */
import {
    MAX_FREQ_HZ,
    MAX_GAIN_DB,
    MIN_FREQ_HZ,
    type EqSectionValues,
    sectionResponseDb,
    typeHasGain,
} from './eqMath';
import type { EqSectionView } from './eqPanelModel';
import { sectionIsModulated } from './eqPanelModel';
import {
    SPECTRUM_CEIL_DB,
    SPECTRUM_FLOOR_DB,
    type SpectrumAnalyzer,
} from './eqSpectrum';

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

export interface EqColors {
    bg: string;
    grid: string;
    text: string;
    curve: string;
    spectrum: string;
    font: string;
}

export function readEqColors(): EqColors {
    const s = getComputedStyle(document.documentElement);
    const v = (name: string, fallback: string) =>
        s.getPropertyValue(name).trim() || fallback;
    return {
        bg: v('--bg-primary', '#0a0a0a'),
        curve: v('--text-primary', '#e6e6e6'),
        font: v('--font-mono', 'monospace'),
        grid: v('--border-subtle', '#222222'),
        spectrum: v('--text-muted', '#8a8a8a'),
        text: v('--text-muted', '#8a8a8a'),
    };
}

/** A section's node as drawn: live values plus its code-side ghost. */
export interface DrawSection {
    view: EqSectionView;
    /** Code-side values diverging from the audio; drawn as a ghost node. */
    ghost: EqSectionView | null;
}

export interface EqDrawState {
    sections: DrawSection[];
    /** Other channels' lists (per-channel form), drawn as faint curves. */
    background: EqSectionView[][];
    selected: number | null;
    hovered: number | null;
    adaptiveQ: boolean;
    sampleRate: number;
    spectrum: SpectrumAnalyzer | null;
}

const FREQ_LABELS: [number, string][] = [
    [50, '50'],
    [100, '100'],
    [200, '200'],
    [500, '500'],
    [1000, '1k'],
    [2000, '2k'],
    [5000, '5k'],
    [10000, '10k'],
];

function drawGrid(
    ctx: CanvasRenderingContext2D,
    g: EqGeometry,
    colors: EqColors,
): void {
    ctx.lineWidth = 1;
    ctx.strokeStyle = colors.grid;
    ctx.beginPath();
    for (let decade = 10; decade <= 10000; decade *= 10) {
        for (let m = 1; m < 10; m++) {
            const f = decade * m;
            if (f < g.fMin || f > g.fMax) {
                continue;
            }
            const x = Math.round(freqToX(g, f)) + 0.5;
            ctx.moveTo(x, g.top);
            ctx.lineTo(x, g.bottom);
        }
    }
    for (const db of [-18, -12, -6, 6, 12, 18]) {
        const y = Math.round(gainToY(g, db)) + 0.5;
        ctx.moveTo(g.left, y);
        ctx.lineTo(g.right, y);
    }
    ctx.stroke();

    ctx.strokeStyle = colors.text;
    ctx.globalAlpha = 0.5;
    ctx.beginPath();
    const y0 = Math.round(gainToY(g, 0)) + 0.5;
    ctx.moveTo(g.left, y0);
    ctx.lineTo(g.right, y0);
    ctx.stroke();
    ctx.globalAlpha = 1;

    ctx.fillStyle = colors.text;
    ctx.font = `10px ${colors.font}`;
    ctx.textBaseline = 'top';
    ctx.textAlign = 'center';
    for (const [f, label] of FREQ_LABELS) {
        if (f <= g.fMax) {
            ctx.fillText(label, freqToX(g, f), g.bottom + 3);
        }
    }
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    for (const db of [12, 0, -12]) {
        ctx.fillText(
            db > 0 ? `+${db}` : String(db),
            g.left - 5,
            gainToY(g, db),
        );
    }
}

function drawSpectrum(
    ctx: CanvasRenderingContext2D,
    g: EqGeometry,
    spectrum: SpectrumAnalyzer,
    colors: EqColors,
    scratch: Float64Array,
): void {
    spectrum.sample(g.freqs, scratch);
    const span = SPECTRUM_CEIL_DB - SPECTRUM_FLOOR_DB;
    const yOf = (db: number) =>
        g.bottom -
        ((Math.min(Math.max(db, SPECTRUM_FLOOR_DB), SPECTRUM_CEIL_DB) -
            SPECTRUM_FLOOR_DB) /
            span) *
            (g.bottom - g.top);
    ctx.beginPath();
    ctx.moveTo(g.left, g.bottom);
    for (let i = 0; i < g.freqs.length; i++) {
        ctx.lineTo(g.left + i, yOf(scratch[i]));
    }
    ctx.lineTo(g.left + g.freqs.length - 1, g.bottom);
    ctx.closePath();
    ctx.fillStyle = colors.spectrum;
    ctx.globalAlpha = 0.18;
    ctx.fill();
    ctx.globalAlpha = 1;
}

function strokeCurve(
    ctx: CanvasRenderingContext2D,
    g: EqGeometry,
    db: Float64Array,
): void {
    ctx.beginPath();
    for (let i = 0; i < db.length; i++) {
        const y = Math.min(
            Math.max(gainToY(g, db[i]), g.top - 2),
            g.bottom + 2,
        );
        if (i === 0) {
            ctx.moveTo(g.left, y);
        } else {
            ctx.lineTo(g.left + i, y);
        }
    }
}

function fillToZero(
    ctx: CanvasRenderingContext2D,
    g: EqGeometry,
    db: Float64Array,
): void {
    strokeCurve(ctx, g, db);
    const y0 = gainToY(g, 0);
    ctx.lineTo(g.left + db.length - 1, y0);
    ctx.lineTo(g.left, y0);
    ctx.closePath();
}

function drawNode(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    index: number,
    opts: {
        selected: boolean;
        hovered: boolean;
        modulated: boolean;
        ghost: boolean;
    },
    colors: EqColors,
): void {
    const color = SECTION_COLORS[index % SECTION_COLORS.length];
    const r = NODE_RADIUS + (opts.hovered || opts.selected ? 1.5 : 0);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    if (opts.ghost) {
        ctx.setLineDash([2, 2]);
        ctx.strokeStyle = color;
        ctx.lineWidth = 1.5;
        ctx.globalAlpha = 0.7;
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.globalAlpha = 1;
        return;
    }
    ctx.fillStyle = color;
    ctx.fill();
    if (opts.modulated) {
        ctx.setLineDash([3, 2]);
        ctx.strokeStyle = colors.curve;
        ctx.lineWidth = 1.5;
        ctx.stroke();
        ctx.setLineDash([]);
    } else if (opts.selected) {
        ctx.strokeStyle = colors.curve;
        ctx.lineWidth = 2;
        ctx.stroke();
    }
    ctx.fillStyle = '#111111';
    ctx.font = `bold 9px ${colors.font}`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(String(index + 1), x, y + 0.5);
}

/** Reusable per-canvas buffers, sized to the geometry. */
export interface EqScratch {
    section: Float64Array;
    sum: Float64Array;
    spectrum: Float64Array;
}

export function makeScratch(g: EqGeometry): EqScratch {
    const n = g.freqs.length;
    return {
        section: new Float64Array(n),
        spectrum: new Float64Array(n),
        sum: new Float64Array(n),
    };
}

/** Sum the responses of a list's constant sections into `scratch.sum`. */
function sumResponse(
    list: EqSectionView[],
    state: EqDrawState,
    g: EqGeometry,
    scratch: EqScratch,
): void {
    scratch.sum.fill(0);
    for (const view of list) {
        const values = knownValues(view);
        if (!values) {
            continue;
        }
        sectionResponseDb(
            values,
            state.adaptiveQ,
            state.sampleRate,
            g.freqs,
            scratch.section,
        );
        for (let k = 0; k < scratch.sum.length; k++) {
            scratch.sum[k] += scratch.section[k];
        }
    }
}

/**
 * Draw the whole graph. A signal-driven section is drawn at its live
 * values with a dashed node; until the first live values arrive it has no
 * response or node.
 */
export function drawEq(
    canvas: HTMLCanvasElement,
    g: EqGeometry,
    scratch: EqScratch,
    state: EqDrawState,
    colors: EqColors,
): void {
    const ctx = canvas.getContext('2d');
    if (!ctx) {
        return;
    }
    const dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, g.width, g.height);
    ctx.fillStyle = colors.bg;
    ctx.fillRect(0, 0, g.width, g.height);

    drawGrid(ctx, g, colors);

    ctx.save();
    ctx.beginPath();
    ctx.rect(g.left, g.top, g.right - g.left, g.bottom - g.top);
    ctx.clip();

    if (state.spectrum) {
        drawSpectrum(ctx, g, state.spectrum, colors, scratch.spectrum);
    }

    for (const list of state.background) {
        sumResponse(list, state, g, scratch);
        strokeCurve(ctx, g, scratch.sum);
        ctx.strokeStyle = colors.curve;
        ctx.globalAlpha = 0.25;
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.globalAlpha = 1;
    }

    scratch.sum.fill(0);
    state.sections.forEach(({ view }, i) => {
        const values = knownValues(view);
        if (!values) {
            return;
        }
        sectionResponseDb(
            values,
            state.adaptiveQ,
            state.sampleRate,
            g.freqs,
            scratch.section,
        );
        for (let k = 0; k < scratch.sum.length; k++) {
            scratch.sum[k] += scratch.section[k];
        }
        if (i === state.selected || i === state.hovered) {
            fillToZero(ctx, g, scratch.section);
            ctx.fillStyle = SECTION_COLORS[i % SECTION_COLORS.length];
            ctx.globalAlpha = i === state.selected ? 0.2 : 0.12;
            ctx.fill();
            ctx.globalAlpha = 1;
        }
    });

    fillToZero(ctx, g, scratch.sum);
    ctx.fillStyle = colors.curve;
    ctx.globalAlpha = 0.06;
    ctx.fill();
    ctx.globalAlpha = 1;
    strokeCurve(ctx, g, scratch.sum);
    ctx.strokeStyle = colors.curve;
    ctx.lineWidth = 2;
    ctx.stroke();

    // The response the code holds, while code-only edits diverge from it.
    if (state.sections.some((s) => s.ghost)) {
        sumResponse(
            state.sections.map((s) => s.ghost ?? s.view),
            state,
            g,
            scratch,
        );
        strokeCurve(ctx, g, scratch.sum);
        ctx.setLineDash([5, 4]);
        ctx.strokeStyle = colors.curve;
        ctx.globalAlpha = 0.6;
        ctx.lineWidth = 1.5;
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.globalAlpha = 1;
    }
    ctx.restore();

    // Ghosts beneath their live nodes, joined by a dashed leader.
    state.sections.forEach(({ view, ghost }, i) => {
        if (!ghost) {
            return;
        }
        const gp = nodePosition(g, ghost);
        if (!gp) {
            return;
        }
        const lp = nodePosition(g, view);
        if (lp) {
            ctx.beginPath();
            ctx.moveTo(lp.x, lp.y);
            ctx.lineTo(gp.x, gp.y);
            ctx.setLineDash([2, 3]);
            ctx.strokeStyle = SECTION_COLORS[i % SECTION_COLORS.length];
            ctx.globalAlpha = 0.5;
            ctx.lineWidth = 1;
            ctx.stroke();
            ctx.setLineDash([]);
            ctx.globalAlpha = 1;
        }
        drawNode(
            ctx,
            gp.x,
            gp.y,
            i,
            {
                ghost: true,
                hovered: false,
                modulated: false,
                selected: false,
            },
            colors,
        );
    });

    const sel =
        state.selected !== null ? state.sections[state.selected]?.view : null;
    const handles = sel && !sel.driven.q ? qHandles(g, sel) : null;
    if (sel && handles) {
        const color = SECTION_COLORS[state.selected! % SECTION_COLORS.length];
        ctx.strokeStyle = color;
        ctx.globalAlpha = 0.45;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(handles[0].x, handles[0].y);
        ctx.lineTo(handles[1].x, handles[1].y);
        ctx.stroke();
        ctx.globalAlpha = 1;
        ctx.fillStyle = color;
        for (const h of handles) {
            ctx.beginPath();
            ctx.roundRect(h.x - 2, h.y - 7, 4, 14, 2);
            ctx.fill();
        }
    }
    const selGhost =
        state.selected !== null ? state.sections[state.selected]?.ghost : null;
    const ghostHandles =
        selGhost && !selGhost.driven.q ? qHandles(g, selGhost) : null;
    if (ghostHandles) {
        ctx.strokeStyle =
            SECTION_COLORS[state.selected! % SECTION_COLORS.length];
        ctx.setLineDash([2, 2]);
        ctx.globalAlpha = 0.8;
        ctx.lineWidth = 1;
        for (const h of ghostHandles) {
            ctx.beginPath();
            ctx.roundRect(h.x - 2.5, h.y - 7.5, 5, 15, 2);
            ctx.stroke();
        }
        ctx.setLineDash([]);
        ctx.globalAlpha = 1;
    }

    state.sections.forEach(({ view }, i) => {
        const p = nodePosition(g, view);
        if (!p) {
            return;
        }
        drawNode(
            ctx,
            p.x,
            p.y,
            i,
            {
                ghost: false,
                hovered: i === state.hovered,
                modulated: sectionIsModulated(view),
                selected: i === state.selected,
            },
            colors,
        );
    });
}
