import type * as MonacoNamespace from 'monaco-editor';
import type { AppTheme } from '../themes/types';
import {
    codeLayout,
    drawCode,
    expandTabs,
    type CodeFrame,
    type CodeLook,
    type CodeRange,
    type CodeSelection,
    type CodeToken,
} from './drawCode';
import type { PerformanceViewSettings } from './settings';
import { TokenColors } from './tokenColors';

type Monaco = typeof MonacoNamespace;
type Editor = MonacoNamespace.editor.IStandaloneCodeEditor;

/** The decoration class the editor gives a pattern step while it plays. */
const PATTERN_STEP_CLASS = 'active-seq-step';
/** Time constant, in milliseconds, of the view easing toward where it should scroll. */
const SCROLL_EASE_MS = 90;
/** Columns kept clear between the cursor and the edge of the view. */
const CURSOR_MARGIN_COLUMNS = 2;

const clamp = (value: number, min: number, max: number) =>
    Math.min(max, Math.max(min, value));

/**
 * The editor's code drawn for an audience: the same text, cursors, selections
 * and playing pattern steps, kept scrolled to what the editor shows, in a look
 * of its own. It draws into a canvas of the performance picture's size.
 */
export class PerformanceCode {
    readonly canvas = document.createElement('canvas');
    private readonly context = this.canvas.getContext('2d');
    private editor: Editor | null = null;
    private subscriptions: MonacoNamespace.IDisposable[] = [];
    private lines: string[] = [];
    private linesStale = true;
    /** The tokens of the first `tokens.length` lines. */
    private tokens: CodeToken[][] = [];
    private tokensStale = true;
    private dirty = true;
    private topLine = 0;
    private leftColumn = 0;
    private lastMs = -1;
    private settings: PerformanceViewSettings | null = null;
    private theme: AppTheme | null = null;
    private colors: TokenColors | null = null;
    private pixelRatio = 0;
    private charWidth = 0;

    constructor(private readonly monaco: Monaco) {}

    /** Follows `editor`, or shows nothing for null. */
    attach(editor: Editor | null): void {
        if (editor === this.editor) return;
        for (const subscription of this.subscriptions) subscription.dispose();
        this.subscriptions = [];
        this.editor = editor;
        this.readModel();
        if (editor === null) return;
        const changed = () => {
            this.dirty = true;
        };
        this.subscriptions = [
            editor.onDidChangeModel(() => this.readModel()),
            editor.onDidChangeModelContent(() => this.readModel()),
            editor.onDidChangeCursorSelection(changed),
            editor.onDidScrollChange(changed),
            editor.onDidLayoutChange(changed),
            editor.onDidChangeModelDecorations(changed),
        ];
    }

    /** Sets the look; `theme` gives the colors. */
    setLook(settings: PerformanceViewSettings, theme: AppTheme): void {
        if (theme !== this.theme) {
            this.theme = theme;
            this.colors = new TokenColors(theme);
        }
        this.settings = settings;
        this.charWidth = 0;
        this.dirty = true;
    }

    /**
     * Redraws at `width` by `height` canvas pixels, at `pixelRatio` canvas
     * pixels per point, if anything has changed. Returns whether it drew.
     */
    render(
        width: number,
        height: number,
        pixelRatio: number,
        nowMs: number,
    ): boolean {
        const { context, settings, theme, colors } = this;
        if (context === null || settings === null || theme === null) {
            return false;
        }
        if (
            this.canvas.width !== width ||
            this.canvas.height !== height ||
            this.pixelRatio !== pixelRatio
        ) {
            this.canvas.width = width;
            this.canvas.height = height;
            this.pixelRatio = pixelRatio;
            this.charWidth = 0;
            this.dirty = true;
        }
        const elapsedMs = this.lastMs < 0 ? Infinity : nowMs - this.lastMs;
        this.lastMs = nowMs;
        if (!this.dirty) return false;

        const fontPx = settings.fontSize * pixelRatio;
        const font = `${fontPx}px "${settings.font}", monospace`;
        if (this.charWidth === 0) {
            context.font = font;
            const sample = '0'.repeat(100);
            this.charWidth = context.measureText(sample).width / 100;
            if (!document.fonts.check(font)) {
                void document.fonts.load(font).then(() => {
                    this.charWidth = 0;
                    this.dirty = true;
                });
            }
        }
        const look: CodeLook = {
            backing: settings.backing,
            charWidth: this.charWidth,
            colors: colors!,
            font: settings.font,
            fontPx,
            ligatures: settings.ligatures,
            lineHeightPx: Math.round(fontPx * settings.lineHeight),
            lineNumbers: settings.lineNumbers,
            theme,
        };
        if (this.linesStale) {
            this.lines = this.editor?.getModel()?.getLinesContent() ?? [];
            this.linesStale = false;
        }
        const layout = codeLayout(look, this.lines.length, width, height);
        const frame = this.frame(layout.visibleLines, layout.visibleColumns);
        const ease = 1 - Math.exp(-elapsedMs / SCROLL_EASE_MS);
        this.topLine += (frame.topLine - this.topLine) * ease;
        this.leftColumn += (frame.leftColumn - this.leftColumn) * ease;
        const settled =
            Math.abs(frame.topLine - this.topLine) < 0.002 &&
            Math.abs(frame.leftColumn - this.leftColumn) < 0.002;
        if (settled) {
            this.topLine = frame.topLine;
            this.leftColumn = frame.leftColumn;
        }
        drawCode(
            context,
            { ...frame, leftColumn: this.leftColumn, topLine: this.topLine },
            look,
            layout,
        );
        this.dirty = !settled;
        return true;
    }

    dispose(): void {
        this.attach(null);
    }

    /**
     * Marks the text out of date. It is read again on the next draw, so edits
     * cost nothing while the code is not drawn.
     */
    private readModel(): void {
        this.linesStale = true;
        this.tokensStale = true;
        this.dirty = true;
    }

    /**
     * What to draw, with the scroll the view should reach: the editor's,
     * moved just enough to keep the cursor in a view of `visibleLines` by
     * `visibleColumns`.
     */
    private frame(visibleLines: number, visibleColumns: number): CodeFrame {
        const { editor, monaco } = this;
        const model = editor?.getModel() ?? null;
        if (editor === null || model === null) {
            return {
                highlights: [],
                leftColumn: 0,
                lines: [],
                selections: [],
                tabSize: 4,
                tokens: null,
                topLine: 0,
            };
        }
        const tabSize = model.getOptions().tabSize;
        const selections: CodeSelection[] = (editor.getSelections() ?? []).map(
            (s) => ({
                active: {
                    column: s.positionColumn - 1,
                    line: s.positionLineNumber - 1,
                },
                end: { column: s.endColumn - 1, line: s.endLineNumber - 1 },
                start: {
                    column: s.startColumn - 1,
                    line: s.startLineNumber - 1,
                },
            }),
        );

        const firstVisible = editor.getVisibleRanges()[0]?.startLineNumber ?? 1;
        const top = editor.getTopForLineNumber(firstVisible);
        const next = editor.getTopForLineNumber(firstVisible + 1);
        let topLine =
            firstVisible -
            1 +
            (next > top
                ? clamp((editor.getScrollTop() - top) / (next - top), 0, 1)
                : 0);
        const { typicalHalfwidthCharacterWidth } = editor.getOption(
            monaco.editor.EditorOption.fontInfo,
        );
        let leftColumn =
            editor.getScrollLeft() / typicalHalfwidthCharacterWidth;

        const cursor = selections[0]?.active;
        if (cursor !== undefined) {
            const fullLines = Math.max(1, Math.floor(visibleLines));
            topLine = clamp(topLine, cursor.line + 1 - fullLines, cursor.line);
            const { columns } = expandTabs(
                this.lines[cursor.line] ?? '',
                tabSize,
            );
            const column = columns[Math.min(cursor.column, columns.length - 1)];
            leftColumn = clamp(
                leftColumn,
                column + CURSOR_MARGIN_COLUMNS - visibleColumns,
                Math.max(0, column - CURSOR_MARGIN_COLUMNS),
            );
        }
        topLine = clamp(topLine, 0, Math.max(0, this.lines.length - 1));
        leftColumn = Math.max(0, leftColumn);

        const first = Math.max(1, Math.floor(topLine) + 1);
        const last = Math.min(
            model.getLineCount(),
            Math.ceil(topLine + visibleLines) + 1,
        );
        // A line's tokens depend on every line before it, so the text is
        // tokenized from the top through the last line in view.
        if (this.tokensStale || last > this.tokens.length) {
            this.tokens = monaco.editor.tokenize(
                model.getValueInRange(
                    new monaco.Range(1, 1, last, model.getLineMaxColumn(last)),
                ),
                model.getLanguageId(),
            );
            this.tokensStale = false;
        }
        const highlights: CodeRange[] = model
            .getDecorationsInRange(
                new monaco.Range(first, 1, last, model.getLineMaxColumn(last)),
            )
            .filter((d) => d.options.className === PATTERN_STEP_CLASS)
            .map(({ range }) => ({
                end: {
                    column: range.endColumn - 1,
                    line: range.endLineNumber - 1,
                },
                start: {
                    column: range.startColumn - 1,
                    line: range.startLineNumber - 1,
                },
            }));

        return {
            highlights,
            leftColumn,
            lines: this.lines,
            selections,
            tabSize,
            tokens: this.tokens,
            topLine,
        };
    }
}
