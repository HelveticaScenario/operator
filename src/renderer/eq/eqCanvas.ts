/**
 * EQ panel graph drawing: response curves (live, other channels, and the
 * code's when it diverges) and Q handles over the painted layers. Geometry
 * is in CSS pixels; drawing scales by devicePixelRatio.
 */
import { sectionResponseDb } from './eqMath';
import type { EqSectionView } from './eqPanelModel';
import { sectionIsModulated } from './eqPanelModel';
import {
    type EqGeometry,
    SECTION_COLORS,
    gainToY,
    knownValues,
    nodePosition,
    qHandles,
} from './eqGeometry';
import {
    type EqColors,
    drawGrid,
    drawNode,
    drawSpectrum,
} from './eqCanvasLayers';
import type { SpectrumAnalyzer } from './eqSpectrum';

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
