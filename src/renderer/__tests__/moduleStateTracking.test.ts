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
 */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import { startModuleStatePolling } from '../components/monaco/moduleStateTracking';

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
 * Minimal Monaco editor/model mock. Tracked decorations are registered by id
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
    let idCounter = 0;
    let collapseThreshold = -1;
    let collectionsCreated = 0;
    let trackedDecorationWrites = 0;

    function makeCollection(initial?: { range: FakeRange }[]) {
        let ids: string[] = [];
        const coll = {
            decos: [] as {
                range: FakeRange;
                options?: { className?: string };
            }[],
            set(
                decos: { range: FakeRange; options?: { className?: string } }[],
            ) {
                for (const id of ids) ranges.delete(id);
                ids = [];
                coll.decos = decos;
                const out: string[] = [];
                for (const d of decos) {
                    const id = `d${idCounter++}`;
                    ids.push(id);
                    ranges.set(id, d.range);
                    out.push(id);
                }
                return out;
            },
            clear() {
                for (const id of ids) ranges.delete(id);
                ids = [];
                coll.decos = [];
            },
        };
        if (initial) coll.set(initial);
        return coll;
    }

    const model = {
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
            newDecos: { range: FakeRange }[],
        ) => {
            for (const id of oldIds) ranges.delete(id);
            if (newDecos.length > 0) trackedDecorationWrites++;
            const out: string[] = [];
            for (const d of newDecos) {
                const id = `d${idCounter++}`;
                ranges.set(id, d.range);
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

    const editor = {
        getModel: () => model,
        createDecorationsCollection: (initial?: { range: FakeRange }[]) => {
            collectionsCreated++;
            return makeCollection(initial);
        },
    };

    const monaco = {
        Range: FakeRange,
        editor: { TrackedRangeStickiness: { NeverGrowsWhenTypingAtEdges: 0 } },
    };

    return {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        editor: editor as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        monaco: monaco as any,
        collapseExisting: () => {
            collapseThreshold = idCounter;
        },
        applyEditShift: (delta: number) => {
            for (const r of ranges.values()) {
                r.startColumn += delta;
                r.endColumn += delta;
            }
        },
        collectionsCreated: () => collectionsCreated,
        trackedDecorationWrites: () => trackedDecorationWrites,
    };
}

function activeDecos(ref: {
    current: {
        decos: { range: FakeRange; options?: { className?: string } }[];
    } | null;
}): { range: FakeRange; options?: { className?: string } }[] {
    return (ref.current?.decos ?? []).filter(
        (d) => d.options?.className === ACTIVE,
    );
}

function activeCount(ref: {
    current: {
        decos: { range: FakeRange; options?: { className?: string } }[];
    } | null;
}): number {
    return activeDecos(ref).length;
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
    afterEach(() => vi.useRealTimers());

    test('same-width edit re-highlights the edited step after its decoration collapses', async () => {
        // `0 1 4` -> `0 7 4`: argument_spans and all_spans are unchanged;
        // only `source` differs. Retyping the middle step collapses its
        // tracked decoration. The edited step must still highlight without a
        // restart.
        const { editor, monaco, collapseExisting } = makeHarness();
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const activeDecorationRef: any = { current: null };

        let state = seqState('0 1 4', [[2, 3]], ALL);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            editor,
            monaco,
            currentFile: 'buf',
            runningBufferId: 'buf',
            activeDecorationRef,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);

        // The user retypes the middle step; its existing decoration collapses.
        collapseExisting();
        state = seqState('0 7 4', [[2, 3]], ALL);

        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);

        stop();
    });

    test('width-change edit (new leaf offsets) highlights the edited step', async () => {
        const { editor, monaco, collapseExisting } = makeHarness();
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const activeDecorationRef: any = { current: null };

        let state = seqState('0 1 4', [[0, 1]], ALL);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            editor,
            monaco,
            currentFile: 'buf',
            runningBufferId: 'buf',
            activeDecorationRef,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);

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
        expect(activeCount(activeDecorationRef)).toBe(1);

        stop();
    });

    test('reposition (argSpan moves, source unchanged) re-anchors the highlight', async () => {
        // Insert lines above the running pattern, then re-eval: the literal's
        // document bounds shift (argSpan.start changes) but `source` is the
        // same. If the tracked decoration drifted/collapsed, only the
        // argSpanChanged re-anchor rebuilds it — `source` alone would not.
        const { editor, monaco, collapseExisting } = makeHarness();
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const activeDecorationRef: any = { current: null };

        let state = seqState('0 1 4', [[2, 3]], ALL, { start: 0, end: 7 });
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            editor,
            monaco,
            currentFile: 'buf',
            runningBufferId: 'buf',
            activeDecorationRef,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);

        // Literal moved down the document; existing decorations drifted.
        collapseExisting();
        state = seqState('0 1 4', [[2, 3]], ALL, { start: 20, end: 27 });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);

        stop();
    });

    test('unchanged source does not rebuild tracked decorations every poll (no churn)', async () => {
        const { editor, monaco, collectionsCreated, trackedDecorationWrites } =
            makeHarness();
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const activeDecorationRef: any = { current: null };

        // Active span moves across steps while source stays the same.
        let state = seqState('0 1 4', [[0, 1]], ALL);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            editor,
            monaco,
            currentFile: 'buf',
            runningBufferId: 'buf',
            activeDecorationRef,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);
        // First poll builds the tracked decorations and the active highlight
        // collection. Capture that baseline; an unchanged `source` must not
        // create any further collections or tracked decorations later.
        const collectionsAfterFirstPoll = collectionsCreated();
        const writesAfterFirstPoll = trackedDecorationWrites();

        // Playhead advances to the next step; source unchanged -> still
        // highlights, and no rebuild.
        state = seqState('0 1 4', [[2, 3]], ALL);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);
        expect(collectionsCreated()).toBe(collectionsAfterFirstPoll);
        expect(trackedDecorationWrites()).toBe(writesAfterFirstPoll);

        state = seqState('0 1 4', [[4, 5]], ALL);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);
        expect(collectionsCreated()).toBe(collectionsAfterFirstPoll);
        expect(trackedDecorationWrites()).toBe(writesAfterFirstPoll);

        stop();
    });
});

describe('startModuleStatePolling anchor lifetime', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

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
        const { editor, monaco, applyEditShift } = makeHarness(DOC);
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const activeDecorationRef: any = { current: null };

        let state = arrangeState([[0, 1]], []);
        const getModuleStates = vi.fn(async () => state);

        const stop = startModuleStatePolling({
            editor,
            monaco,
            currentFile: 'buf',
            runningBufferId: 'buf',
            activeDecorationRef,
            getModuleStates,
            pollInterval: 50,
        });

        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);

        // The user inserts 5 characters before the patch code; tracked
        // decorations follow, raw evaluation-time offsets do not.
        applyEditShift(5);

        // The arrange advances to the second section.
        state = arrangeState([], [[0, 1]]);
        await vi.advanceTimersByTimeAsync(50);

        const decos = activeDecos(activeDecorationRef);
        expect(decos).toHaveLength(1);
        // Anchored at evaluate time (content offset 0 of 'c d'), then shifted
        // with the edit — not resolved from the stale offset after the edit.
        expect(decos[0].range.startColumn).toBe(P1 + 2 + 5);

        stop();
    });

    test('anchors survive a polling restart and keep following edits', async () => {
        const { editor, monaco, applyEditShift, trackedDecorationWrites } =
            makeHarness(DOC);
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const activeDecorationRef: any = { current: null };

        let state = arrangeState([[0, 1]], []);
        const getModuleStates = vi.fn(async () => state);

        const params = {
            editor,
            monaco,
            currentFile: 'buf',
            runningBufferId: 'buf',
            activeDecorationRef,
            getModuleStates,
            pollInterval: 50,
        };

        const stop = startModuleStatePolling(params);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);
        const writesAfterFirstSession = trackedDecorationWrites();
        stop();

        // Edit while the polling effect is restarting (e.g. a tab switch).
        applyEditShift(5);

        const stop2 = startModuleStatePolling(params);
        state = arrangeState([[0, 1]], []);
        await vi.advanceTimersByTimeAsync(50);

        const decos = activeDecos(activeDecorationRef);
        expect(decos).toHaveLength(1);
        // The restarted session reuses the live tracked anchors (shifted by
        // the edit) rather than re-anchoring from evaluation-time offsets.
        expect(decos[0].range.startColumn).toBe(P0 + 2 + 5);
        expect(trackedDecorationWrites()).toBe(writesAfterFirstSession);

        stop2();
    });
});
