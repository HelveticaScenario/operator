import type { CodeLook } from './drawCode';

/** What a control call's widget shows. */
export type ControlWidget =
    | {
          kind: 'slider';
          /** The value's place in the slider's range, 0 to 1. */
          fraction: number;
      }
    | { kind: 'button'; on: boolean };

/** A widget drawn after the text of `line`, the 0-based line its call starts on. */
export type InlineControl = ControlWidget & { line: number };

/** Width of a slider's track, in columns. */
const SLIDER_COLUMNS = 8;
/** Width of a button, in columns. */
const BUTTON_COLUMNS = 2;
/** Columns between the end of a line's text and its first widget, and between widgets. */
const GAP_COLUMNS = 1;

/** Width of `control`'s widget, in columns. */
function columnsOf(control: ControlWidget): number {
    return control.kind === 'slider' ? SLIDER_COLUMNS : BUTTON_COLUMNS;
}

/** Columns the widgets of one line take after the end of its text. */
export function lineControlsColumns(
    controls: readonly ControlWidget[],
): number {
    return controls.reduce(
        (total, control) => total + GAP_COLUMNS + columnsOf(control),
        0,
    );
}

/**
 * Draws `controls`, all on one line whose top is at `y`, in order from `x`,
 * the right edge of the line's text, each on a backing of its own.
 */
export function drawLineControls(
    ctx: CanvasRenderingContext2D,
    controls: readonly InlineControl[],
    x: number,
    y: number,
    look: CodeLook,
): void {
    const { charWidth, lineHeightPx } = look;
    const { colors } = look.theme;
    const midY = y + lineHeightPx / 2;
    let left = x + GAP_COLUMNS * charWidth;
    for (const control of controls) {
        const width = columnsOf(control) * charWidth;
        if (look.backing > 0) {
            ctx.globalAlpha = look.backing;
            ctx.fillStyle = colors.bgPrimary;
            ctx.fillRect(
                left - charWidth / 2,
                y,
                width + charWidth,
                lineHeightPx,
            );
            ctx.globalAlpha = 1;
        }
        if (control.kind === 'slider') {
            const fraction = Number.isFinite(control.fraction)
                ? Math.min(1, Math.max(0, control.fraction))
                : 0;
            const track = Math.max(2, lineHeightPx * 0.14);
            const knob = lineHeightPx * 0.22;
            const from = left + knob;
            const span = width - 2 * knob;
            ctx.fillStyle = colors.textMuted;
            ctx.fillRect(from, midY - track / 2, span, track);
            ctx.fillStyle = colors.accentPrimary;
            ctx.fillRect(from, midY - track / 2, span * fraction, track);
            ctx.beginPath();
            ctx.arc(from + span * fraction, midY, knob, 0, 2 * Math.PI);
            ctx.fill();
        } else {
            const radius = Math.min(width, lineHeightPx) * 0.3;
            ctx.beginPath();
            ctx.arc(left + width / 2, midY, radius, 0, 2 * Math.PI);
            if (control.on) {
                ctx.fillStyle = colors.accentPrimary;
                ctx.fill();
            } else {
                ctx.strokeStyle = colors.textMuted;
                ctx.lineWidth = Math.max(1, radius * 0.3);
                ctx.stroke();
            }
        }
        left += width + GAP_COLUMNS * charWidth;
    }
}
