/**
 * Tests for scope view zone creation. A view without an anchor — a null
 * decorationIndex, or a tracked decoration range that is empty or missing —
 * has no anchor line in the document, so it must get no view zone and no
 * registered canvas — matching the removal path in repositionZones — rather
 * than a zone anchored at the top of the file.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { editor } from 'monaco-editor';
import { createScopeViewZones } from './scopeViewZones';
import type { ScopeView } from '../../types/editor';

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

function makeElement() {
    return {
        style: {} as Record<string, string>,
        dataset: {} as Record<string, string>,
        className: '',
        clientWidth: 0,
        width: 0,
        height: 0,
        appendChild: () => {},
    };
}

function makeEditor() {
    const addedZones: editor.IViewZone[] = [];
    const ed = {
        getLayoutInfo: () => ({ contentWidth: 800 }),
        changeViewZones: (
            cb: (accessor: {
                addZone: (z: editor.IViewZone) => string;
                removeZone: (id: string) => void;
                layoutZone: (id: string) => void;
            }) => void,
        ) => {
            cb({
                addZone: (z) => {
                    addedZones.push(z);
                    return `z${addedZones.length}`;
                },
                removeZone: () => {},
                layoutZone: () => {},
            });
        },
        onDidLayoutChange: () => ({ dispose: () => {} }),
    };
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return { editor: ed as any, addedZones };
}

function makeView(key: string, decorationIndex: number | null): ScopeView {
    return {
        channelKeys: [key],
        decorationIndex,
        file: 'buf',
        key,
        range: [-5, 5],
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any;
}

function makeDecorations(rangesByIndex: (FakeRange | null)[]) {
    return {
        getRange: (index: number) => rangesByIndex[index] ?? null,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any;
}

describe('createScopeViewZones', () => {
    beforeEach(() => {
        vi.stubGlobal('document', { createElement: () => makeElement() });
    });
    afterEach(() => {
        vi.unstubAllGlobals();
    });

    test('a view with a collapsed decoration range gets no zone and no canvas', () => {
        const { editor, addedZones } = makeEditor();
        const registered: string[] = [];

        const handle = createScopeViewZones({
            editor,
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            monaco: {} as any,
            views: [makeView('a', 0), makeView('b', 1)],
            scopeDecorations: makeDecorations([
                new FakeRange(3, 1, 3, 10),
                // Deleted `.scope()` call: the tracked range collapsed.
                new FakeRange(5, 4, 5, 4),
            ]),
            onRegisterScopeCanvas: (key) => registered.push(key),
        });

        expect(addedZones).toHaveLength(1);
        expect(addedZones[0].afterLineNumber).toBe(3);
        expect(registered).toEqual(['a']);

        handle.dispose();
    });

    test('an anchorless view is skipped without shifting later views onto its decoration', () => {
        const { editor, addedZones } = makeEditor();
        const registered: string[] = [];

        // 'b' has no anchor (unresolvable call site) and owns no decoration;
        // 'c' owns decoration index 1 and must resolve to it even though the
        // views array positions no longer line up with decoration indexes.
        const handle = createScopeViewZones({
            editor,
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            monaco: {} as any,
            views: [makeView('a', 0), makeView('b', null), makeView('c', 1)],
            scopeDecorations: makeDecorations([
                new FakeRange(3, 1, 3, 10),
                new FakeRange(7, 1, 7, 10),
            ]),
            onRegisterScopeCanvas: (key) => registered.push(key),
        });

        expect(addedZones).toHaveLength(2);
        expect(addedZones[0].afterLineNumber).toBe(3);
        expect(addedZones[1].afterLineNumber).toBe(7);
        expect(registered).toEqual(['a', 'c']);

        handle.dispose();
    });

    test('a null decoration collection creates no zones at all', () => {
        const { editor, addedZones } = makeEditor();
        const registered: string[] = [];

        const handle = createScopeViewZones({
            editor,
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            monaco: {} as any,
            views: [makeView('a', 0), makeView('b', 1)],
            scopeDecorations: null,
            onRegisterScopeCanvas: (key) => registered.push(key),
        });

        expect(addedZones).toHaveLength(0);
        expect(registered).toEqual([]);

        handle.dispose();
    });
});
