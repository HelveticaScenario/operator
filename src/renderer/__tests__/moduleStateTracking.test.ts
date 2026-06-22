/**
 * Regression tests for pattern step highlighting ($p / $p.s).
 *
 * Bug: with a pattern running, editing one step and re-evaluating left the
 * edited step un-highlighted ("a node already present highlights, but a new
 * note disappears") until stop/restart.
 *
 * Ground truth from the native module: a same-width value edit (e.g. `1`->`7`)
 * leaves both `argument_spans` and `all_spans` IDENTICAL — only the evaluated
 * `source` changes. The tracked Monaco decoration for the edited step was built
 * once and only rebuilt when the argument bounds changed, so retyping the step
 * (NeverGrowsWhenTypingAtEdges) collapsed that decoration to an empty range and
 * the step stopped highlighting. The fix rebuilds tracked decorations whenever
 * `source` changes, restoring the edited step's highlight from fresh spans.
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
 * Minimal Monaco editor/model mock. Decoration collections record their ranges
 * by id so `getDecorationRange` resolves them, mirroring Monaco's tracked
 * decorations.
 *
 * `collapseExisting()` models a text edit that collapses every decoration that
 * already exists to an empty range (what NeverGrowsWhenTypingAtEdges does when
 * the user replaces a span's content). Decorations created afterward are valid.
 */
function makeHarness() {
    const ranges = new Map<string, FakeRange>();
    let idCounter = 0;
    let collapseThreshold = -1;
    let collectionsCreated = 0;

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
        // Non-interpolated literal: contains no '${'.
        getValueInRange: () => '"0 2 4"',
        getDecorationRange: (id: string) => {
            const createdAt = Number(id.slice(1));
            if (createdAt < collapseThreshold) {
                // Edited-over decoration collapsed to zero width.
                return new FakeRange(1, 1, 1, 1);
            }
            return ranges.get(id) ?? null;
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
        collectionsCreated: () => collectionsCreated,
    };
}

function activeCount(ref: {
    current: { decos: { options?: { className?: string } }[] } | null;
}): number {
    return (ref.current?.decos ?? []).filter(
        (d) => d.options?.className === ACTIVE,
    ).length;
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
        // The reported case: `0 1 4` -> `0 7 4`. argument_spans and all_spans
        // are unchanged; only `source` differs. Retyping the middle step
        // collapses its tracked decoration. The edited step must still highlight
        // without a restart.
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
        // would fire on its own), but pinning argSpan isolates the new
        // source-change trigger: without it the stale span map lacks the "2:4"
        // id and the edited step never re-highlights. This test fails without
        // the sourceChanged rebuild.
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
        const { editor, monaco, collectionsCreated } = makeHarness();
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
        // First poll builds the tracked-decoration collection and the active
        // highlight collection. Capture that baseline; an unchanged `source`
        // must not create any further collections on later polls.
        const afterFirstPoll = collectionsCreated();

        // Playhead advances to the next step; source unchanged -> still
        // highlights, and no rebuild (collection count must not grow).
        state = seqState('0 1 4', [[2, 3]], ALL);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);
        expect(collectionsCreated()).toBe(afterFirstPoll);

        state = seqState('0 1 4', [[4, 5]], ALL);
        await vi.advanceTimersByTimeAsync(50);
        expect(activeCount(activeDecorationRef)).toBe(1);
        expect(collectionsCreated()).toBe(afterFirstPoll);

        stop();
    });
});
