/**
 * Tests for pattern step highlighting ($p / $p.s) via startModuleStatePolling.
 *
 * Invariants guarded:
 * - Tracked decorations anchor every leaf span on the first poll after a
 *   patch evaluates — `argument_spans` are evaluation-time offsets, so
 *   anchoring must happen before edits can shift the document — including
 *   params with no currently active spans.
 * - Anchors persist across polling restarts (cache keyed by model) and keep
 *   following edits instead of being re-derived from stale evaluation-time
 *   offsets.
 * - A change to a pattern's evaluated `source` rebuilds its tracked
 *   decorations, so an edited step highlights without a restart. Ground truth
 *   from the native module: a same-width value edit (e.g. `1` -> `7`) leaves
 *   both `argument_spans` and `all_spans` identical — only `source` changes —
 *   while the retype collapses the step's tracked decoration
 *   (NeverGrowsWhenTypingAtEdges).
 * - Interpolated patterns redirect highlights into const literals: positions
 *   in a nested template const's literal text re-map from evaluated to raw
 *   offsets, and boundary positions are biased so a span start resolves into
 *   the region that begins there and a span end into the region that ends
 *   there.
 * - Step highlights live on the running buffer's model, whether or not an
 *   editor shows it: a stopped session leaves none behind, and they move to
 *   the new model when the running buffer changes.
 */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import { startModuleStatePolling } from '../components/monaco/moduleStateTracking';
import { setActiveInterpolationResolutions } from '../../shared/dsl/spanTypes';

const ACTIVE = 'active-seq-step';

class FakeRange {
    constructor(
        public startLineNumber: number,
        public startColumn: number,
        public endLineNumber: number,
        public endColumn: number,
    ) {}
    isEmpty(): boolean {
        return (
            this.startLineNumber === this.endLineNumber &&
            this.startColumn === this.endColumn
        );
    }
}

/**
 * Minimal Monaco model mock. Decorations are registered by id
 * (via `model.deltaDecorations`) so `getDecorationRange` resolves them,
 * mirroring Monaco's model-owned tracked decorations. Documents are single
 * line, so offset N maps to column N+1.
 *
 * `collapseExisting()` models a text edit that collapses every decoration that
 * already exists to an empty range (what NeverGrowsWhenTypingAtEdges does when
 * the user replaces a span's content). Decorations created afterward are valid.
 *
 * `applyEditShift(delta)` models an insertion above/before all decorations:
 * every existing decoration range shifts by `delta` columns (as Monaco's
 * tracking does), while raw offsets from module state do not.
 */
function makeHarness(docText?: string) {
    const ranges = new Map<string, FakeRange>();
    const classNames = new Map<string, string | undefined>();
    let idCounter = 0;
    let collapseThreshold = -1;
    let trackedDecorationWrites = 0;

    const model = {
        isDisposed: () => false,
        getPositionAt: (offset: number) => ({
            lineNumber: 1,
            column: offset + 1,
        }),
        getValueInRange: (r: { startColumn: number; endColumn: number }) =>
            docText
                ? docText.slice(r.startColumn - 1, r.endColumn - 1)
                : '"0 2 4"',
        deltaDecorations: (
            oldIds: string[],
            newDecos: { range: FakeRange; options?: { className?: string } }[],
        ) => {
            for (const id of oldIds) {
                ranges.delete(id);
                classNames.delete(id);
            }
            if (newDecos.some((d) => d.options?.className !== ACTIVE)) {
                trackedDecorationWrites++;
            }
            const out: string[] = [];
            for (const d of newDecos) {
                const id = `d${idCounter++}`;
                ranges.set(id, d.range);
                classNames.set(id, d.options?.className);
                out.push(id);
            }
            return out;
        },
        getDecorationRange: (id: string) => {
            const createdAt = Number(id.slice(1));
            if (createdAt < collapseThreshold) {
                // Edited-over decoration collapsed to zero width.
                return new FakeRange(1, 1, 1, 1);
            }
            const r = ranges.get(id);
            // Snapshot (Monaco returns a fresh Range) so a stored range is
            // never registered under a second id and shifted twice.
            return r
                ? new FakeRange(
                      r.startLineNumber,
                      r.startColumn,
                      r.endLineNumber,
                      r.endColumn,
                  )
                : null;
        },
    };

    const monaco = {
        Range: FakeRange,
        editor: { TrackedRangeStickiness: { NeverGrowsWhenTypingAtEdges: 0 } },
    };

    return {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        model: model as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        monaco: monaco as any,
        /** The step highlights on the model. */
        activeDecos: (): { range: FakeRange }[] =>
            [...ranges]
                .filter(([id]) => classNames.get(id) === ACTIVE)
                .map(([, range]) => ({ range })),
        collapseExisting: () => {
            collapseThreshold = idCounter;
        },
        applyEditShift: (delta: number) => {
            for (const r of ranges.values()) {
                r.startColumn += delta;
                r.endColumn += delta;
            }
        },
        trackedDecorationWrites: () => trackedDecorationWrites,
    };
}

function seqState(
    source: string,
    activeSpans: [number, number][],
    allSpans: [number, number][],
    argSpan: { start: number; end: number } = { start: 0, end: 7 },
): Record<string, unknown> {
    return {
        seq1: {
            argument_spans: { pattern: argSpan },
            param_spans: {
                pattern: { spans: activeSpans, source, all_spans: allSpans },
            },
        },
    };
}

const ALL = [
    [0, 1],
    [2, 3],
    [4, 5],
] as [number, number][];

describe('startModuleStatePolling step highlighting', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => {
        vi.useRealTimers();
        setActiveInterpolationResolutions(null);
    });

    test('same-width edit re-highlights the edited step after its decoration collapses', async () => {
        // `0 1 4` -> `0 7 4`: argument_spans and all_spans are unchanged;
        // only `source` differs. Retyping the middle step collapses its
        // tracked decoration. The edited step must still highlight without a
        // restart.
        const { model, monaco, activeDecos, collapseExisting } = makeHarness();

        let state = seqState('0 1 4', [[2, 3]], ALL);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);

        // The user retypes the middle step; its existing decoration collapses.
        collapseExisting();
        state = seqState('0 7 4', [[2, 3]], ALL);

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);

        stop();
    });

    test('width-change edit (new leaf offsets) highlights the edited step', async () => {
        const { model, monaco, activeDecos, collapseExisting } = makeHarness();

        let state = seqState('0 1 4', [[0, 1]], ALL);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);

        // `0 1 4` -> `0 33 4`: middle step widens; offsets shift, edited step
        // now playing at the new [2,4] span.
        //
        // argSpan is deliberately left unchanged here. In the real editor a
        // widening edit also moves the literal's end bound (argSpanChanged
        // would fire on its own), but pinning argSpan isolates the
        // source-change trigger: without it the stale span map lacks the "2:4"
        // id and the edited step never re-highlights.
        collapseExisting();
        state = seqState(
            '0 33 4',
            [[2, 4]],
            [
                [0, 1],
                [2, 4],
                [5, 6],
            ],
        );

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);

        stop();
    });

    test('reposition (argSpan moves, source unchanged) re-anchors the highlight', async () => {
        // Insert lines above the running pattern, then re-eval: the literal's
        // document bounds shift (argSpan.start changes) but `source` is the
        // same. If the tracked decoration drifted/collapsed, only the
        // argSpanChanged re-anchor rebuilds it — `source` alone would not.
        const { model, monaco, activeDecos, collapseExisting } = makeHarness();

        let state = seqState('0 1 4', [[2, 3]], ALL, { start: 0, end: 7 });
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);

        // Literal moved down the document; existing decorations drifted.
        collapseExisting();
        state = seqState('0 1 4', [[2, 3]], ALL, { start: 20, end: 27 });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);

        stop();
    });

    test('unchanged source does not rebuild tracked decorations every poll (no churn)', async () => {
        const { model, monaco, activeDecos, trackedDecorationWrites } =
            makeHarness();

        // Active span moves across steps while source stays the same.
        let state = seqState('0 1 4', [[0, 1]], ALL);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);
        // First poll builds the tracked decorations. Capture that baseline;
        // an unchanged `source` must not create any further tracked
        // decorations later.
        const writesAfterFirstPoll = trackedDecorationWrites();

        // Playhead advances to the next step; source unchanged -> still
        // highlights, and no rebuild.
        state = seqState('0 1 4', [[2, 3]], ALL);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);
        expect(trackedDecorationWrites()).toBe(writesAfterFirstPoll);

        state = seqState('0 1 4', [[4, 5]], ALL);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);
        expect(trackedDecorationWrites()).toBe(writesAfterFirstPoll);

        stop();
    });
});

describe('startModuleStatePolling anchor lifetime', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => {
        vi.useRealTimers();
        setActiveInterpolationResolutions(null);
    });

    const DOC = "$p.arrange([8, 'a b'], [8, 'c d'])";
    const P0 = DOC.indexOf("'a b'");
    const P1 = DOC.indexOf("'c d'");

    function arrangeState(
        p0Spans: [number, number][],
        p1Spans: [number, number][],
    ): Record<string, unknown> {
        return {
            seq1: {
                argument_spans: {
                    'pattern.0': { start: P0, end: P0 + 5 },
                    'pattern.1': { start: P1, end: P1 + 5 },
                },
                param_spans: {
                    'pattern.0': {
                        spans: p0Spans,
                        source: 'a b',
                        all_spans: [
                            [0, 1],
                            [2, 3],
                        ],
                    },
                    'pattern.1': {
                        spans: p1Spans,
                        source: 'c d',
                        all_spans: [
                            [0, 1],
                            [2, 3],
                        ],
                    },
                },
            },
        };
    }

    test('a param with no active spans anchors on the first poll, so its highlights follow later edits', async () => {
        // Arrange section 'pattern.1' is silent for its first 8 cycles: its
        // active spans are empty while 'pattern.0' plays. Its anchors must
        // still be created on the first poll — an edit made before the
        // section first activates must not misplace its highlights.
        const { model, monaco, activeDecos, applyEditShift } = makeHarness(DOC);

        let state = arrangeState([[0, 1]], []);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);

        // The user inserts 5 characters before the patch code; tracked
        // decorations follow, raw evaluation-time offsets do not.
        applyEditShift(5);

        // The arrange advances to the second section.
        state = arrangeState([], [[0, 1]]);
        await vi.advanceTimersByTimeAsync(50);

        const decos = activeDecos();
        expect(decos).toHaveLength(1);
        // Anchored at evaluate time (content offset 0 of 'c d'), then shifted
        // with the edit — not resolved from the stale offset after the edit.
        expect(decos[0].range.startColumn).toBe(P1 + 2 + 5);

        stop();
    });

    test('anchors survive a polling restart and keep following edits', async () => {
        const {
            model,
            monaco,
            activeDecos,
            applyEditShift,
            trackedDecorationWrites,
        } = makeHarness(DOC);

        let state = arrangeState([[0, 1]], []);
        const getModuleStates = vi.fn(async () => state);

        const params = {
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        };

        const stop = startModuleStatePolling(params);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos().length).toBe(1);
        const writesAfterFirstSession = trackedDecorationWrites();
        stop();

        // Edit while the polling effect is restarting (e.g. a tab switch).
        applyEditShift(5);

        const stop2 = startModuleStatePolling(params);
        state = arrangeState([[0, 1]], []);
        await vi.advanceTimersByTimeAsync(50);

        const decos = activeDecos();
        expect(decos).toHaveLength(1);
        // The restarted session reuses the live tracked anchors (shifted by
        // the edit) rather than re-anchoring from evaluation-time offsets.
        expect(decos[0].range.startColumn).toBe(P0 + 2 + 5);
        expect(trackedDecorationWrites()).toBe(writesAfterFirstSession);

        stop2();
    });

    test('stopping a session clears its highlights, including a poll still in flight', async () => {
        const { model, monaco, activeDecos } = makeHarness(DOC);
        const state = arrangeState([[0, 1]], []);
        let release = () => {};
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });
        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos()).toHaveLength(1);

        // The next poll waits on module states while the session stops.
        getModuleStates.mockImplementationOnce(
            () =>
                new Promise((resolve) => {
                    release = () => resolve(state);
                }),
        );
        await vi.advanceTimersByTimeAsync(50);
        stop();
        expect(activeDecos()).toHaveLength(0);
        release();
        await vi.advanceTimersByTimeAsync(50);
        expect(activeDecos()).toHaveLength(0);
    });

    test('highlights move to the new model when the running buffer changes', async () => {
        const first = makeHarness(DOC);
        const second = makeHarness(DOC);
        let running = first.model;
        const state = arrangeState([[0, 1]], []);

        const stop = startModuleStatePolling({
            getModel: () => running,
            monaco: first.monaco,
            getModuleStates: vi.fn(async () => state),
            pollInterval: 50,
        });
        await vi.advanceTimersByTimeAsync(50);
        expect(first.activeDecos()).toHaveLength(1);

        running = second.model;
        await vi.advanceTimersByTimeAsync(50);
        expect(first.activeDecos()).toHaveLength(0);
        expect(second.activeDecos()).toHaveLength(1);

        stop();
    });
});

describe('startModuleStatePolling interpolation resolution', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => {
        vi.useRealTimers();
        setActiveInterpolationResolutions(null);
    });

    test('positions in a nested template const literal re-map through its interpolation regions', async () => {
        // chord is itself a template: its raw `${root}` (7 chars) evaluates
        // to 'c4' (2 chars), so positions in chord's literal tail sit 5 chars
        // later in the raw source than in the evaluated result.
        const DOC =
            "const root = 'c4'; const chord = `${root} e4 g4`; $p(`${chord} b4`)";
        const rootStart = DOC.indexOf("'c4'");
        const chordStart = DOC.indexOf('`${root} e4 g4`');
        const argStart = DOC.indexOf('`${chord} b4`');
        const argSpan = { start: argStart, end: argStart + 13 };

        setActiveInterpolationResolutions(
            new Map([
                [
                    `${argSpan.start}:${argSpan.end}`,
                    [
                        {
                            evaluatedStart: 0,
                            evaluatedLength: 8,
                            constLiteralSpan: {
                                start: chordStart,
                                end: chordStart + 15,
                            },
                            nestedResolutions: [
                                {
                                    evaluatedStart: 0,
                                    evaluatedLength: 2,
                                    constLiteralSpan: {
                                        start: rootStart,
                                        end: rootStart + 4,
                                    },
                                },
                            ],
                        },
                    ],
                ],
            ]),
        );

        const { model, monaco, activeDecos } = makeHarness(DOC);

        // Evaluated pattern: 'c4 e4 g4 b4'. Leaf 'e4' = [3,5] lives in
        // chord's literal tail; leaf 'c4' = [0,2] lives in root's result.
        const allSpans: [number, number][] = [
            [0, 2],
            [3, 5],
            [6, 8],
            [9, 11],
        ];
        let state = seqState('c4 e4 g4 b4', [[3, 5]], allSpans, argSpan);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        let decos = activeDecos();
        expect(decos).toHaveLength(1);
        let text = DOC.slice(
            decos[0].range.startColumn - 1,
            decos[0].range.endColumn - 1,
        );
        expect(text).toBe('e4');
        // 'e4' sits at chord content offset 8 in the raw literal, not at its
        // evaluated offset 3.
        expect(decos[0].range.startColumn - 1).toBe(chordStart + 1 + 8);

        // A position inside the nested interpolation's result redirects all
        // the way to the root const literal.
        state = seqState('c4 e4 g4 b4', [[0, 2]], allSpans, argSpan);
        await vi.advanceTimersByTimeAsync(50);
        decos = activeDecos();
        expect(decos).toHaveLength(1);
        text = DOC.slice(
            decos[0].range.startColumn - 1,
            decos[0].range.endColumn - 1,
        );
        expect(text).toBe('c4');
        expect(decos[0].range.startColumn - 1).toBe(rootStart + 1);

        stop();
    });

    test('a span starting on the boundary of adjacent interpolations resolves into the const that begins there', async () => {
        const DOC = "const a = 'c4 '; const b = 'e4'; $p(`${a}${b}`)";
        const aStart = DOC.indexOf("'c4 '");
        const bStart = DOC.indexOf("'e4'");
        const argStart = DOC.indexOf('`${a}${b}`');
        const argSpan = { start: argStart, end: argStart + 10 };

        setActiveInterpolationResolutions(
            new Map([
                [
                    `${argSpan.start}:${argSpan.end}`,
                    [
                        {
                            evaluatedStart: 0,
                            evaluatedLength: 3,
                            constLiteralSpan: {
                                start: aStart,
                                end: aStart + 5,
                            },
                        },
                        {
                            evaluatedStart: 3,
                            evaluatedLength: 2,
                            constLiteralSpan: {
                                start: bStart,
                                end: bStart + 4,
                            },
                        },
                    ],
                ],
            ]),
        );

        const { model, monaco, activeDecos } = makeHarness(DOC);

        // Evaluated pattern: 'c4 e4'. Leaf 'e4' = [3,5] starts exactly where
        // a's result ends and b's result begins. Its highlight must cover
        // only b's literal — never span from const a to const b.
        const state = seqState(
            'c4 e4',
            [[3, 5]],
            [
                [0, 2],
                [3, 5],
            ],
            argSpan,
        );
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            getModel: () => model,
            monaco,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        const decos = activeDecos();
        expect(decos).toHaveLength(1);
        const text = DOC.slice(
            decos[0].range.startColumn - 1,
            decos[0].range.endColumn - 1,
        );
        expect(text).toBe('e4');
        expect(decos[0].range.startColumn - 1).toBe(bStart + 1);
        expect(decos[0].range.endColumn - 1).toBe(bStart + 3);

        stop();
    });
});
