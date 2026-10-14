import type * as MonacoNamespace from 'monaco-editor';
import { ITALIC_FONTS } from '../../shared/fonts';
import type { AppTheme } from '../themes/types';
import {
    codeLayout,
    drawCode,
    expandTabs,
    type CodeFrame,
    type CodeLook,
    type ColoredRange,
    type CodeRange,
    type CodeSelection,
    type CodeToken,
} from './drawCode';
import {
    lineControlsColumns,
    type ControlWidget,
    type InlineControl,
} from './drawControls';
import { scrollEaseMs, type PerformanceViewSettings } from './settings';
import { TokenColors } from './tokenColors';

type Monaco = typeof MonacoNamespace;
type Editor = MonacoNamespace.editor.IStandaloneCodeEditor;
type Model = MonacoNamespace.editor.ITextModel;

/** The decoration class the editor gives a pattern step while it plays. */
const PATTERN_STEP_CLASS = 'active-seq-step';
/** The decoration classes of the editor's bracket pair colorization. */
const BRACKET_LEVEL_CLASS = /^bracket-highlighting-(\d+)$/;
const UNEXPECTED_BRACKET_CLASS = 'unexpected-closing-bracket';
/** Columns kept clear between the cursor and the edge of the view. */
const CURSOR_MARGIN_COLUMNS = 2;
/** Monaco's TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges. */
const NEVER_GROWS_WHEN_TYPING_AT_EDGES = 1;

/** A control call's widget, placed by the offset of its callee name in the code. */
export type PlacedControl = ControlWidget & { offset: number };

/** Where the editor has the code scrolled, and its selections there. */
interface EditorView {
    selections: CodeSelection[];
    topLine: number;
    leftColumn: number;
}

/** Calls `listener` on each wheel event over `editor`. */
function onWheel(
    editor: Editor,
    listener: () => void,
): MonacoNamespace.IDisposable {
    const node = editor.getDomNode();
    node?.addEventListener('wheel', listener, { passive: true });
    return { dispose: () => node?.removeEventListener('wheel', listener) };
}

const clamp = (value: number, min: number, max: number) =>
    Math.min(max, Math.max(min, value));

/**
 * A buffer's code drawn for an audience: its text, playing pattern steps and
 * control widgets, with the editor's cursors, selections and scroll while the
 * editor shows it, in a look of its own. It draws into a canvas of the
 * performance picture's size.
 */
export class PerformanceCode {
    readonly canvas = document.createElement('canvas');
    private readonly context = this.canvas.getContext('2d');
    private editor: Editor | null = null;
    private editorSubscriptions: MonacoNamespace.IDisposable[] = [];
    /** The model path of the buffer shown, or null to show the editor's. */
    private bufferPath: string | null = null;
    private model: Model | null = null;
    private modelSubscriptions: MonacoNamespace.IDisposable[] = [];
    private readonly createdModel: MonacoNamespace.IDisposable;
    /** The editor's view of the model, kept from when it last showed it. */
    private editorView: EditorView | null = null;
    private lines: string[] = [];
    private linesStale = true;
    /** The tokens of the first `tokens.length` lines. */
    private tokens: CodeToken[][] = [];
    private tokensStale = true;
    private dirty = true;
    private topLine = 0;
    private leftColumn = 0;
    private lastMs = -1;
    private controls: readonly PlacedControl[] = [];
    /**
     * A tracked decoration on the model at a control call the view keeps in
     * sight in place of the cursor, until the user moves the editor.
     */
    private revealed: string | null = null;
    private settings: PerformanceViewSettings | null = null;
    private theme: AppTheme | null = null;
    private colors: TokenColors | null = null;
    private pixelRatio = 0;
    private charWidth = 0;

    constructor(private readonly monaco: Monaco) {
        // A buffer's model is created when the editor first shows it.
        this.createdModel = monaco.editor.onDidCreateModel(() =>
            this.syncModel(),
        );
    }

    /** Takes the cursors, selections and scroll from `editor` while it shows the code. */
    attach(editor: Editor | null): void {
        if (editor === this.editor) return;
        for (const subscription of this.editorSubscriptions) {
            subscription.dispose();
        }
        this.editorSubscriptions = [];
        this.editor = editor;
        this.syncModel();
        this.dirty = true;
        if (editor === null) return;
        const changed = () => {
            if (this.showsModel()) this.dirty = true;
        };
        const moved = () => {
            if (!this.showsModel()) return;
            this.clearReveal();
            this.dirty = true;
        };
        const { RecoverFromMarkers } = this.monaco.editor.CursorChangeReason;
        this.editorSubscriptions = [
            editor.onDidChangeModel(() => {
                this.syncModel();
                this.dirty = true;
            }),
            // An edit elsewhere in the text shifts the cursor without the
            // user moving it.
            editor.onDidChangeCursorSelection((e) =>
                e.reason === RecoverFromMarkers ? changed() : moved(),
            ),
            // The editor scrolls itself on a resize or as view zones change
            // height, so only the user's wheel and clicks count as moves.
            onWheel(editor, moved),
            editor.onMouseDown(moved),
            editor.onDidScrollChange(changed),
            editor.onDidLayoutChange(changed),
        ];
    }

    /**
     * Shows the buffer whose model is at `path`, or the editor's buffer for
     * null. A buffer the editor has not shown yet appears once it does.
     */
    showBuffer(path: string | null): void {
        if (path === this.bufferPath) return;
        this.bufferPath = path;
        this.syncModel();
    }

    /** Sets the control widgets, placed in the text of the buffer shown. */
    setControls(controls: readonly PlacedControl[]): void {
        this.controls = controls;
        this.dirty = true;
    }

    /**
     * Scrolls the view to the control call at `offset` in the buffer shown,
     * keeping the call and its widget in sight as it would the cursor,
     * leaving the editor as it is. The view goes back to following the
     * cursor when the user moves the editor.
     */
    reveal(offset: number): void {
        const { model } = this;
        if (model === null) return;
        const position = model.getPositionAt(offset);
        [this.revealed] = model.deltaDecorations(
            this.revealed === null ? [] : [this.revealed],
            [
                {
                    options: { stickiness: NEVER_GROWS_WHEN_TYPING_AT_EDGES },
                    range: new this.monaco.Range(
                        position.lineNumber,
                        position.column,
                        position.lineNumber,
                        position.column,
                    ),
                },
            ],
        );
        this.dirty = true;
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
            italicFace: ITALIC_FONTS.has(settings.font),
            ligatures: settings.ligatures,
            lineHeightPx: Math.round(fontPx * settings.lineHeight),
            lineNumbers: settings.lineNumbers,
            theme,
        };
        if (this.linesStale) {
            this.lines = this.model?.getLinesContent() ?? [];
            this.linesStale = false;
        }
        const layout = codeLayout(look, this.lines.length, width, height);
        const frame = this.frame(layout.visibleLines, layout.visibleColumns);
        const easeMs = scrollEaseMs(settings.scrollSpeed);
        const ease = easeMs === 0 ? 1 : 1 - Math.exp(-elapsedMs / easeMs);
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
        this.createdModel.dispose();
        this.attach(null);
        this.setModel(null);
    }

    /** Whether the editor shows the code drawn. */
    private showsModel(): boolean {
        return this.model !== null && this.editor?.getModel() === this.model;
    }

    /** Follows the model of the buffer shown. */
    private syncModel(): void {
        const model =
            this.bufferPath === null
                ? (this.editor?.getModel() ?? null)
                : this.monaco.editor.getModel(
                      this.monaco.Uri.parse(this.bufferPath),
                  );
        this.setModel(model);
    }

    private setModel(model: Model | null): void {
        if (model === this.model) return;
        this.clearReveal();
        for (const subscription of this.modelSubscriptions) {
            subscription.dispose();
        }
        this.model = model;
        this.editorView = null;
        this.readModel();
        this.modelSubscriptions =
            model === null
                ? []
                : [
                      model.onDidChangeContent(() => this.readModel()),
                      model.onDidChangeDecorations(() => {
                          this.dirty = true;
                      }),
                      model.onWillDispose(() => this.setModel(null)),
                  ];
    }

    private clearReveal(): void {
        if (this.revealed === null) return;
        if (this.model !== null && !this.model.isDisposed()) {
            this.model.deltaDecorations([this.revealed], []);
        }
        this.revealed = null;
    }

    /** The color of a bracket the editor decorates with `className`, if it is one. */
    private bracketColor(
        className: string | null | undefined,
    ): string | undefined {
        if (this.colors === null || !className) return undefined;
        if (className === UNEXPECTED_BRACKET_CLASS) {
            return this.colors.bracket(null);
        }
        const level = BRACKET_LEVEL_CLASS.exec(className);
        return level === null
            ? undefined
            : this.colors.bracket(Number(level[1]));
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

    /** The editor's selections and scroll, while it shows the code. */
    private readEditorView(editor: Editor): EditorView {
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
        const topLine =
            firstVisible -
            1 +
            (next > top
                ? clamp((editor.getScrollTop() - top) / (next - top), 0, 1)
                : 0);
        const { typicalHalfwidthCharacterWidth } = editor.getOption(
            this.monaco.editor.EditorOption.fontInfo,
        );
        const leftColumn =
            editor.getScrollLeft() / typicalHalfwidthCharacterWidth;
        return { leftColumn, selections, topLine };
    }

    /**
     * What to draw, with the scroll the view should reach: the editor's,
     * moved just enough to keep the cursor, or a revealed control call and
     * its widget, in a view of `visibleLines` by `visibleColumns`.
     */
    private frame(visibleLines: number, visibleColumns: number): CodeFrame {
        const { editor, model, monaco } = this;
        if (model === null) {
            return {
                brackets: [],
                controls: [],
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
        if (editor !== null && this.showsModel()) {
            this.editorView = this.readEditorView(editor);
        }
        const { selections, ...scroll } = this.editorView ?? {
            leftColumn: 0,
            selections: [],
            topLine: 0,
        };
        let { leftColumn, topLine } = scroll;

        const controls = this.controls.map(
            ({ offset, ...widget }): InlineControl => ({
                ...widget,
                line: model.getPositionAt(offset).lineNumber - 1,
            }),
        );

        // The place to keep in sight: a line, and columns from `start` to
        // `end` with tabs expanded.
        let focus: { line: number; start: number; end: number } | null = null;
        const revealed =
            this.revealed === null
                ? null
                : model.getDecorationRange(this.revealed);
        if (revealed !== null) {
            const line = revealed.startLineNumber - 1;
            const { columns } = expandTabs(this.lines[line] ?? '', tabSize);
            focus = {
                end:
                    columns[columns.length - 1] +
                    lineControlsColumns(
                        controls.filter((c) => c.line === line),
                    ),
                line,
                start: columns[
                    Math.min(revealed.startColumn - 1, columns.length - 1)
                ],
            };
        } else if (selections[0] !== undefined) {
            const { line, column } = selections[0].active;
            const { columns } = expandTabs(this.lines[line] ?? '', tabSize);
            const at = columns[Math.min(column, columns.length - 1)];
            focus = { end: at, line, start: at };
        }
        if (focus !== null) {
            const fullLines = Math.max(1, Math.floor(visibleLines));
            topLine = clamp(topLine, focus.line + 1 - fullLines, focus.line);
            leftColumn = Math.min(
                leftColumn,
                Math.max(0, focus.start - CURSOR_MARGIN_COLUMNS),
            );
            // Past the width of the view, the end of the focus stays in sight.
            leftColumn = Math.max(
                leftColumn,
                focus.end + CURSOR_MARGIN_COLUMNS - visibleColumns,
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
        const highlights: CodeRange[] = [];
        const brackets: ColoredRange[] = [];
        for (const { options, range } of model.getDecorationsInRange(
            new monaco.Range(first, 1, last, model.getLineMaxColumn(last)),
        )) {
            const codeRange = {
                end: {
                    column: range.endColumn - 1,
                    line: range.endLineNumber - 1,
                },
                start: {
                    column: range.startColumn - 1,
                    line: range.startLineNumber - 1,
                },
            };
            if (options.className === PATTERN_STEP_CLASS) {
                highlights.push(codeRange);
                continue;
            }
            const color = this.bracketColor(options.inlineClassName);
            if (color !== undefined) brackets.push({ ...codeRange, color });
        }

        return {
            brackets,
            controls,
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
