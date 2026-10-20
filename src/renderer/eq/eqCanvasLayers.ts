/**
 * The EQ graph's painted layers that don't depend on the response: theme
 * colors, the frequency/gain grid, the spectrum fill, and section nodes.
 */
import {
    type EqGeometry,
    NODE_RADIUS,
    SECTION_COLORS,
    freqToX,
    gainToY,
} from './eqGeometry';
import {
    SPECTRUM_CEIL_DB,
    SPECTRUM_FLOOR_DB,
    type SpectrumAnalyzer,
} from './eqSpectrum';

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

export function drawGrid(
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

export function drawSpectrum(
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

export function drawNode(
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
