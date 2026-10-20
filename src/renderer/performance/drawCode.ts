import type { AppTheme } from '../themes/types';
import { drawLineControls, type InlineControl } from './drawControls';
import type { TokenColors, TokenStyle } from './tokenColors';

/** A token from Monaco's tokenizer: where it starts in its line, and its type. */
export interface CodeToken {
    offset: number;
    type: string;
}

/** A place in the code: 0-based line, and 0-based character index in that line. */
export interface CodePosition {
    line: number;
    column: number;
}

export interface CodeRange {
    start: CodePosition;
    end: CodePosition;
}

/** A range drawn in a color of its own. */
export interface ColoredRange extends CodeRange {
    color: string;
}

/** A selection, with `active` the end the cursor is at. */
export interface CodeSelection extends CodeRange {
    active: CodePosition;
}

/** What the editor holds at one moment, and where the view onto it is scrolled. */
export interface CodeFrame {
    lines: readonly string[];
    /** Tokens per line, or null before the code has been tokenized. */
    tokens: readonly (readonly CodeToken[])[] | null;
    tabSize: number;
    /** The line at the top of the view, fractional while it scrolls. */
    topLine: number;
    /** The column at the left of the text, in columns with tabs expanded. */
    leftColumn: number;
    selections: readonly CodeSelection[];
    /** Pattern steps playing now. */
    highlights: readonly CodeRange[];
    /** Brackets the editor colors by nesting level, each on one line, with their colors. */
    brackets: readonly ColoredRange[];
    /** Widgets of the control calls, in the order of the calls. */
    controls: readonly InlineControl[];
}

/** How the code is drawn, in canvas pixels. */
export interface CodeLook {
    font: string;
    fontPx: number;
    lineHeightPx: number;
    /** Width of one column of the monospace font. */
    charWidth: number;
    ligatures: boolean;
    /** Whether the font has an italic face; without one, italic tokens draw upright. */
    italicFace: boolean;
    lineNumbers: boolean;
    /** Opacity of the backing behind each line, and of the cursor line's highlight. */
    backing: number;
    /** The background of a playing pattern step, as CSS. */
    stepBackground: string;
    theme: AppTheme;
    colors: TokenColors;
}

/** Where the code sits in a canvas of a given size. */
export interface CodeLayout {
    width: number;
    height: number;
    /** Left edge of the line numbers. */
    gutterX: number;
    /** Left edge of the text. */
    textX: number;
    /** Top edge of the first line when scrolled to the top. */
    padY: number;
    /** Lines that fit, counting a partly shown one. */
    visibleLines: number;
    /** Columns of text that fit. */
    visibleColumns: number;
}

export function codeLayout(
    look: CodeLook,
    lineCount: number,
    width: number,
    height: number,
): CodeLayout {
    const pad = look.charWidth * 2;
    const padY = look.lineHeightPx * 0.5;
    const gutter = look.lineNumbers
        ? (String(Math.max(1, lineCount)).length + 2) * look.charWidth
        : 0;
    const textX = pad + gutter;
    return {
        gutterX: pad,
        height,
        padY,
        textX,
        visibleColumns: Math.max(1, (width - textX - pad) / look.charWidth),
        visibleLines: Math.max(1, (height - 2 * padY) / look.lineHeightPx),
        width,
    };
}

/**
 * A line with its tabs expanded to spaces, and the column each UTF-16 index of
 * the line starts at, which is also its index in `text`; `columns[line.length]`
 * is the line's width in columns. A character outside the Basic Multilingual
 * Plane, such as an emoji, spans two columns, as it does in `text`.
 */
export function expandTabs(
    line: string,
    tabSize: number,
): { text: string; columns: number[] } {
    const columns: number[] = [];
    let text = '';
    for (let i = 0; i < line.length; i++) {
        columns.push(text.length);
        text +=
            line[i] === '\t'
                ? ' '.repeat(tabSize - (text.length % tabSize))
                : line[i];
    }
    columns.push(text.length);
    return { columns, text };
}

/** The 0-based character spans of line `line` that `ranges` cover. */
export function spansOnLine(
    ranges: readonly CodeRange[],
    line: number,
    lineLength: number,
): [number, number][] {
    const spans: [number, number][] = [];
    for (const { start, end } of ranges) {
        if (line < start.line || line > end.line) continue;
        const from = line === start.line ? start.column : 0;
        const to = line === end.line ? end.column : lineLength;
        if (to > from) spans.push([from, to]);
    }
    return spans;
}

/**
 * Runs of a line that share one style: token boundaries split by bracket
 * edges. A run inside a bracket carries the bracket's color.
 */
export function styledRuns(
    length: number,
    tokens: readonly CodeToken[],
    brackets: readonly [number, number, string][] = [],
): {
    from: number;
    to: number;
    token: number;
    bracket: string | null;
}[] {
    const cuts = new Set<number>([0, length]);
    for (const { offset } of tokens) cuts.add(Math.min(offset, length));
    for (const [from, to] of brackets) {
        cuts.add(Math.min(from, length));
        cuts.add(Math.min(to, length));
    }
    const sorted = [...cuts].sort((a, b) => a - b);
    const runs: ReturnType<typeof styledRuns> = [];
    let token = -1;
    for (let k = 0; k + 1 < sorted.length; k++) {
        const from = sorted[k];
        const to = sorted[k + 1];
        while (token + 1 < tokens.length && tokens[token + 1].offset <= from) {
            token++;
        }
        runs.push({
            bracket:
                brackets.find(([a, b]) => from >= a && to <= b)?.[2] ?? null,
            from,
            to,
            token,
        });
    }
    return runs;
}

const fontOf = (look: CodeLook, style: Pick<TokenStyle, 'bold' | 'italic'>) =>
    `${style.italic && look.italicFace ? 'italic ' : ''}${style.bold ? 'bold ' : ''}${look.fontPx}px "${look.font}", monospace`;

/** Draws `frame` onto a cleared, transparent canvas. */
export function drawCode(
    ctx: CanvasRenderingContext2D,
    frame: CodeFrame,
    look: CodeLook,
    layout: CodeLayout,
): void {
    const { colors: theme } = look.theme;
    const { charWidth, lineHeightPx } = look;
    ctx.clearRect(0, 0, layout.width, layout.height);
    ctx.textBaseline = 'middle';

    const first = Math.max(0, Math.floor(frame.topLine));
    const last = Math.min(
        frame.lines.length - 1,
        Math.ceil(frame.topLine + layout.visibleLines),
    );
    const yOf = (line: number) =>
        layout.padY + (line - frame.topLine) * lineHeightPx;
    const xOf = (column: number) =>
        layout.textX + (column - frame.leftColumn) * charWidth;
    const cursorLines = new Set(frame.selections.map((s) => s.active.line));
    // A bracket is a single character, so it never spans lines.
    const bracketsByLine = new Map<number, [number, number, string][]>();
    for (const { start, end, color } of frame.brackets) {
        const span: [number, number, string] = [
            start.column,
            end.column,
            color,
        ];
        const onLine = bracketsByLine.get(start.line);
        if (onLine) onLine.push(span);
        else bracketsByLine.set(start.line, [span]);
    }
    const controlsByLine = new Map<number, InlineControl[]>();
    for (const control of frame.controls) {
        const onLine = controlsByLine.get(control.line);
        if (onLine) onLine.push(control);
        else controlsByLine.set(control.line, [control]);
    }

    for (let line = first; line <= last; line++) {
        const y = yOf(line);
        const { text, columns } = expandTabs(frame.lines[line], frame.tabSize);
        const width = columns[columns.length - 1];

        // The cursor line's highlight covers the same span as the line's
        // backing, over it.
        if (look.backing > 0 && (width > 0 || look.lineNumbers)) {
            const left = look.lineNumbers ? layout.gutterX : layout.textX;
            const right =
                width > 0 ? Math.max(layout.textX, xOf(width)) : layout.textX;
            ctx.globalAlpha = look.backing;
            for (const fill of cursorLines.has(line)
                ? [theme.bgPrimary, theme.editorLineHighlight]
                : [theme.bgPrimary]) {
                ctx.fillStyle = fill;
                ctx.fillRect(
                    left - charWidth / 2,
                    y,
                    right - left + charWidth,
                    lineHeightPx,
                );
            }
            ctx.globalAlpha = 1;
        }
        if (look.lineNumbers) {
            ctx.font = fontOf(look, { bold: false, italic: false });
            ctx.fillStyle = cursorLines.has(line)
                ? theme.lineNumberActiveForeground
                : theme.lineNumberForeground;
            ctx.textAlign = 'right';
            ctx.fillText(
                String(line + 1),
                layout.textX - charWidth,
                y + lineHeightPx / 2,
            );
            ctx.textAlign = 'left';
        }

        // The text scrolls under the line numbers, so it is clipped at them.
        ctx.save();
        ctx.beginPath();
        ctx.rect(layout.textX, 0, layout.width - layout.textX, layout.height);
        ctx.clip();
        ctx.fillStyle = theme.editorSelection;
        for (const { start, end } of frame.selections) {
            if (line < start.line || line > end.line) continue;
            const from = columns[line === start.line ? start.column : 0];
            // A selection running on to the next line covers the line break.
            const to = line === end.line ? columns[end.column] : width + 1;
            if (to > from) {
                ctx.fillRect(
                    xOf(from),
                    y,
                    (to - from) * charWidth,
                    lineHeightPx,
                );
            }
        }
        // A playing step is a box behind the text, as the editor draws its
        // step decoration; the text keeps its own style.
        ctx.fillStyle = look.stepBackground;
        for (const [from, to] of spansOnLine(
            frame.highlights,
            line,
            frame.lines[line].length,
        )) {
            ctx.fillRect(
                xOf(columns[from]),
                y,
                (columns[to] - columns[from]) * charWidth,
                lineHeightPx,
            );
        }

        const tokens = frame.tokens?.[line] ?? [];
        const brackets = bracketsByLine.get(line) ?? [];
        for (const run of styledRuns(
            frame.lines[line].length,
            tokens,
            brackets,
        )) {
            const token = tokens[run.token];
            const tokenStyle = look.colors.style(token?.type ?? '');
            const style =
                run.bracket === null
                    ? tokenStyle
                    : { ...tokenStyle, color: run.bracket };
            ctx.font = fontOf(look, style);
            ctx.fillStyle = style.color;
            const textY = y + lineHeightPx / 2;
            const piece = text.slice(columns[run.from], columns[run.to]);
            if (look.ligatures) {
                ctx.fillText(piece, xOf(columns[run.from]), textY);
            } else {
                let k = 0;
                for (const char of piece) {
                    ctx.fillText(char, xOf(columns[run.from] + k), textY);
                    k += char.length;
                }
            }
        }
        const controls = controlsByLine.get(line);
        if (controls) {
            drawLineControls(ctx, controls, xOf(width), y, look);
        }
        ctx.restore();
    }

    ctx.fillStyle = theme.editorCursor;
    const caret = Math.max(2, Math.round(look.fontPx * 0.08));
    for (const { active } of frame.selections) {
        if (active.line < first || active.line > last) continue;
        const { columns } = expandTabs(frame.lines[active.line], frame.tabSize);
        const x = xOf(columns[Math.min(active.column, columns.length - 1)]);
        if (x < layout.textX) continue;
        ctx.fillRect(x, yOf(active.line), caret, lineHeightPx);
    }
}
