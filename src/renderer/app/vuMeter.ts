/**
 * VU meter bar rendering + display ballistics, in the visual language of a
 * DAW mixer strip: thin per-channel bars on a dark track, a peak-hold
 * triangle with dash markers, and a dB scale down the right side whose
 * tick/label density adapts to the meter's pixel height.
 *
 * The engine ships per-channel RMS (300 ms EMA) and a windowed peak in volts;
 * everything display-related — dB conversion, peak hold/release, drawing —
 * happens here so the audio thread stays maths-only.
 */

export const VU_FLOOR_DB = -60;
export const VU_CEIL_DB = 6;

/** Peak-hold duration before the marker starts falling. */
const PEAK_HOLD_MS = 1500;
/** Peak marker release rate once the hold expires. */
const PEAK_RELEASE_DB_PER_S = 24;
/** Release rate of the fast (dim) meter layer — snappy but still animated. */
const FAST_RELEASE_DB_PER_S = 96;

/**
 * The scale is piecewise-linear in dB: the musically busy +6…−24 dB region
 * gets this fraction of the height, the −24…−60 dB tail the remainder.
 */
const SCALE_BREAK_DB = -24;
const SCALE_BREAK_FRAC = 0.6;

/** 0 dB reference is 5 V (full scale in the engine's voltage convention). */
export function voltsToDb(v: number): number {
    return 20 * Math.log10(Math.max(v, 1e-6) / 5);
}

/** Readout-pill text: "-∞" at the floor, else a signed dB value. */
export function formatDb(db: number): string {
    if (db <= VU_FLOOR_DB + 0.5) {
        return '-∞';
    }
    const rounded = Math.round(db * 10) / 10;
    return Number.isInteger(rounded)
        ? String(rounded)
        : rounded.toFixed(1);
}

/** Per-channel meter display state, advanced once per drawn frame: the
 *  fast (dim) layer and the held peak marker above it. */
export interface VuBallistics {
    displayPeakDb: number;
    peakHoldUntil: number;
    displayFastDb: number;
    lastMs: number;
}

export function newBallistics(): VuBallistics {
    return {
        displayFastDb: VU_FLOOR_DB,
        displayPeakDb: VU_FLOOR_DB,
        lastMs: 0,
        peakHoldUntil: 0,
    };
}

/**
 * Advance the display layers from this frame's engine peak. The fast layer
 * rises instantly and releases at FAST_RELEASE_DB_PER_S; the peak marker
 * rises instantly, holds for PEAK_HOLD_MS, then releases at
 * PEAK_RELEASE_DB_PER_S.
 */
export function updateBallistics(
    b: VuBallistics,
    peakDb: number,
    nowMs: number,
): void {
    const dt = b.lastMs > 0 ? (nowMs - b.lastMs) / 1000 : 0;
    b.lastMs = nowMs;

    if (peakDb >= b.displayFastDb) {
        b.displayFastDb = peakDb;
    } else {
        b.displayFastDb = Math.max(
            peakDb,
            b.displayFastDb - FAST_RELEASE_DB_PER_S * dt,
        );
    }
    if (b.displayFastDb < VU_FLOOR_DB) {
        b.displayFastDb = VU_FLOOR_DB;
    }

    if (peakDb >= b.displayPeakDb) {
        b.displayPeakDb = peakDb;
        b.peakHoldUntil = nowMs + PEAK_HOLD_MS;
    } else if (nowMs > b.peakHoldUntil) {
        const elapsed = (nowMs - b.peakHoldUntil) / 1000;
        b.displayPeakDb = Math.max(
            peakDb,
            b.displayPeakDb - PEAK_RELEASE_DB_PER_S * elapsed,
        );
        // Release is applied from the hold point each frame; advancing the
        // anchor keeps the fall linear rather than accelerating.
        b.peakHoldUntil = nowMs;
    }
    if (b.displayPeakDb < VU_FLOOR_DB) {
        b.displayPeakDb = VU_FLOOR_DB;
    }
}

export interface VuMeterColors {
    bg: string;
    track: string;
    border: string;
    muted: string;
    low: string;
    mid: string;
    hot: string;
    /** Externally-controlled (signal-driven, locked) control accents. */
    external: string;
}

export function readVuMeterColors(): VuMeterColors {
    const styles = getComputedStyle(document.documentElement);
    return {
        bg: styles.getPropertyValue('--bg-primary').trim() || '#0a0a0a',
        track: '#141414',
        border: styles.getPropertyValue('--border-subtle').trim() || '#222222',
        muted: styles.getPropertyValue('--text-muted').trim() || '#8a8a8a',
        low: styles.getPropertyValue('--color-success').trim() || '#9fd94f',
        mid: styles.getPropertyValue('--color-warning').trim() || '#d7ba4a',
        hot: styles.getPropertyValue('--color-error').trim() || '#e05561',
        external:
            styles.getPropertyValue('--accent-primary').trim() || '#4ec9b0',
    };
}

/** CSS padding above/below the meter scale inside the canvas. */
export const METER_PAD_CSS = 5;

/** Fraction of the scale height (0 = top) for a dB value. */
function dbToNorm(db: number): number {
    const clamped = Math.min(Math.max(db, VU_FLOOR_DB), VU_CEIL_DB);
    if (clamped >= SCALE_BREAK_DB) {
        return (
            ((VU_CEIL_DB - clamped) / (VU_CEIL_DB - SCALE_BREAK_DB)) *
            SCALE_BREAK_FRAC
        );
    }
    return (
        SCALE_BREAK_FRAC +
        ((SCALE_BREAK_DB - clamped) / (SCALE_BREAK_DB - VU_FLOOR_DB)) *
            (1 - SCALE_BREAK_FRAC)
    );
}

/** Inverse of dbToNorm, for mapping fader-drag positions back to dB. */
export function meterNormToDb(norm: number): number {
    const clamped = Math.min(Math.max(norm, 0), 1);
    if (clamped <= SCALE_BREAK_FRAC) {
        return (
            VU_CEIL_DB -
            (clamped / SCALE_BREAK_FRAC) * (VU_CEIL_DB - SCALE_BREAK_DB)
        );
    }
    return (
        SCALE_BREAK_DB -
        ((clamped - SCALE_BREAK_FRAC) / (1 - SCALE_BREAK_FRAC)) *
            (SCALE_BREAK_DB - VU_FLOOR_DB)
    );
}

/**
 * Label sets by priority tier. Tiers are added whole while every label in
 * the resulting set keeps a readable pixel gap, so a short meter shows
 * 0/24/48 and a tall one the full 6-dB ladder.
 */
const LABEL_TIERS: number[][] = [
    [0, -24, -48],
    [-12, -36, -60],
    [-6, -18],
    [6, -30, -42, -54],
];

function pickLabels(scaleHeight: number, minGap: number): number[] {
    let chosen = LABEL_TIERS[0];
    for (let tier = 1; tier < LABEL_TIERS.length; tier++) {
        const candidate = [...chosen, ...LABEL_TIERS[tier]].sort(
            (a, b) => b - a,
        );
        let ok = true;
        for (let i = 1; i < candidate.length; i++) {
            const gap =
                (dbToNorm(candidate[i]) - dbToNorm(candidate[i - 1])) *
                scaleHeight;
            if (gap < minGap) {
                ok = false;
                break;
            }
        }
        if (!ok) {
            break;
        }
        chosen = candidate;
    }
    return [...chosen].sort((a, b) => b - a);
}

/** Draw a rounded rectangle path. */
function roundRect(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
): void {
    const radius = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + radius, y);
    ctx.arcTo(x + w, y, x + w, y + h, radius);
    ctx.arcTo(x + w, y + h, x, y + h, radius);
    ctx.arcTo(x, y + h, x, y, radius);
    ctx.arcTo(x, y, x + w, y, radius);
    ctx.closePath();
}

/**
 * Draw one meter's channels (post-ballistics): the gain-fader triangle on
 * the left, thin per-channel bars with peak-hold dashes on a rounded dark
 * track, and an adaptive dB scale on the right. `gainDb` positions the
 * fader triangle (null/undefined hides it); `gainLocked` recolors it to
 * mark a signal-driven, externally-controlled gain. `ghostGainDb` draws a
 * second, faded triangle at the gain the source code holds when it
 * diverges from the running audio. The canvas backing store is
 * device-pixel sized by the caller.
 */
export function drawVuMeter(
    canvas: HTMLCanvasElement,
    channels: Array<{ rmsDb: number; fastDb: number; peakDb: number }>,
    colors: VuMeterColors,
    gainDb?: number | null,
    gainLocked?: boolean,
    ghostGainDb?: number | null,
): void {
    const ctx = canvas.getContext('2d');
    if (!ctx) {
        return;
    }
    const w = canvas.width;
    const h = canvas.height;
    const dpr = window.devicePixelRatio || 1;

    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = colors.bg;
    ctx.fillRect(0, 0, w, h);

    // Horizontal layout: triangle gutter | bars | tick dashes | labels.
    const labelW = 15 * dpr;
    const tickW = 6 * dpr;
    const triW = 9 * dpr;
    const padY = 5 * dpr;
    const scaleH = h - padY * 2;
    const barsLeft = triW;
    const barsRight = w - labelW - tickW;
    const barsW = barsRight - barsLeft;
    if (barsW <= 0 || scaleH <= 0) {
        return;
    }

    const dbToY = (db: number) => padY + dbToNorm(db) * scaleH;

    // Track behind the bars.
    ctx.fillStyle = colors.track;
    roundRect(ctx, barsLeft, padY, barsW, scaleH, 2 * dpr);
    ctx.fill();
    ctx.strokeStyle = colors.border;
    ctx.lineWidth = 1;
    ctx.stroke();

    // Thin per-channel bars, centered in the track.
    const barW = 3.5 * dpr;
    const barGap = 2.5 * dpr;
    const totalBarsW =
        channels.length * barW + (channels.length - 1) * barGap;
    const firstBarX = barsLeft + (barsW - totalBarsW) / 2;
    const yZero = dbToY(0);
    const yMid = dbToY(-12);
    const yBottom = padY + scaleH;

    // Zone-segmented level fill: green to −12 dB, warning to 0, error
    // above. A signal-controlled gain tints the whole bar in the external
    // accent instead, flipping back to zone colors when control returns.
    const fillLevel = (x: number, db: number) => {
        const yTop = dbToY(db);
        if (yTop >= yBottom) {
            return;
        }
        if (gainLocked) {
            ctx.fillStyle = colors.external;
            ctx.fillRect(x, yTop, barW, yBottom - yTop);
            return;
        }
        const greenTop = Math.max(yTop, yMid);
        ctx.fillStyle = colors.low;
        ctx.fillRect(x, greenTop, barW, yBottom - greenTop);
        if (yTop < yMid) {
            const midTop = Math.max(yTop, yZero);
            ctx.fillStyle = colors.mid;
            ctx.fillRect(x, midTop, barW, yMid - midTop);
            if (yTop < yZero) {
                ctx.fillStyle = colors.hot;
                ctx.fillRect(x, yTop, barW, yZero - yTop);
            }
        }
    };

    for (let ch = 0; ch < channels.length; ch++) {
        const x = firstBarX + ch * (barW + barGap);
        const { rmsDb, fastDb, peakDb } = channels[ch];

        // Unlit bar body, always visible like a hardware meter's dark glass.
        ctx.fillStyle = '#000000';
        ctx.fillRect(x, padY, barW, scaleH);

        // Fast layer first, dimmed; the smooth layer paints over its lower
        // span, leaving the dim segment visible between the two levels.
        ctx.globalAlpha = 0.45;
        fillLevel(x, fastDb);
        ctx.globalAlpha = 1;
        fillLevel(x, rmsDb);

        // Per-channel peak-hold dash on the bar.
        if (peakDb > VU_FLOOR_DB) {
            const yPeak = dbToY(peakDb);
            ctx.fillStyle = peakDb > 0 ? colors.hot : '#d8d8d8';
            ctx.fillRect(x, yPeak - dpr, barW, 2 * dpr);
        }
    }

    // Gain-fader triangles in the left gutter (−Infinity parks at the
    // floor): faded ghost at the source-code value first, live audio value
    // on top.
    const fillGainTriangle = (db: number) => {
        const yGain = dbToY(Math.max(db, VU_FLOOR_DB));
        const triH = 4 * dpr;
        ctx.beginPath();
        ctx.moveTo(barsLeft - 8 * dpr, yGain - triH);
        ctx.lineTo(barsLeft - 2 * dpr, yGain);
        ctx.lineTo(barsLeft - 8 * dpr, yGain + triH);
        ctx.closePath();
        ctx.fill();
    };
    if (ghostGainDb !== null && ghostGainDb !== undefined) {
        ctx.fillStyle = '#d8d8d8';
        ctx.globalAlpha = 0.35;
        fillGainTriangle(ghostGainDb);
        ctx.globalAlpha = 1;
    }
    if (gainDb !== null && gainDb !== undefined) {
        ctx.fillStyle = gainLocked ? colors.external : '#d8d8d8';
        fillGainTriangle(gainDb);
    }

    // dB scale: minor dashes every 6 dB when there's room, labels by tier.
    ctx.strokeStyle = colors.muted;
    ctx.fillStyle = colors.muted;
    ctx.font = `${9 * dpr}px ${getComputedStyle(document.documentElement).getPropertyValue('--font-mono') || 'monospace'}`;
    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';

    const labels = new Set(pickLabels(scaleH, 13 * dpr));
    const sixDbGap = (dbToNorm(-30) - dbToNorm(-24)) * scaleH;
    const drawMinor = sixDbGap >= 5 * dpr;
    for (let db = VU_CEIL_DB; db >= VU_FLOOR_DB; db -= 6) {
        const y = dbToY(db);
        const labeled = labels.has(db);
        if (!labeled && !drawMinor) {
            continue;
        }
        ctx.globalAlpha = labeled ? 0.9 : 0.45;
        ctx.beginPath();
        ctx.moveTo(barsRight + 1.5 * dpr, y);
        ctx.lineTo(barsRight + (labeled ? 4 : 3) * dpr, y);
        ctx.stroke();
        if (labeled) {
            // DAW convention: magnitudes only, the sign is implied.
            ctx.fillText(
                String(Math.abs(db)),
                barsRight + tickW + 1.5 * dpr,
                y,
            );
        }
    }
    ctx.globalAlpha = 1;
}
