import { describe, expect, test } from 'vitest';

import { type AnchorModel, createControlAnchors } from '../controlAnchors';

/**
 * A text model whose decorations track edits like Monaco's: a range shifts
 * with edits before it, and an edit inside it changes its length.
 */
class FakeModel implements AnchorModel {
    private decorations = new Map<string, { start: number; end: number }>();
    private nextId = 0;
    disposed = false;

    constructor(public text: string) {}

    private lineStarts(): number[] {
        const starts = [0];
        for (let i = 0; i < this.text.length; i++) {
            if (this.text[i] === '\n') {
                starts.push(i + 1);
            }
        }
        return starts;
    }

    getLineCount() {
        return this.lineStarts().length;
    }

    getLineContent(lineNumber: number) {
        return this.text.split('\n')[lineNumber - 1];
    }

    getOffsetAt(position: { lineNumber: number; column: number }) {
        return this.lineStarts()[position.lineNumber - 1] + position.column - 1;
    }

    private positionAt(offset: number) {
        const starts = this.lineStarts();
        let line = 0;
        while (line + 1 < starts.length && starts[line + 1] <= offset) {
            line++;
        }
        return { column: offset - starts[line] + 1, lineNumber: line + 1 };
    }

    getDecorationRange(id: string) {
        const d = this.decorations.get(id);
        if (!d) {
            return null;
        }
        const start = this.positionAt(d.start);
        const end = this.positionAt(d.end);
        return {
            endColumn: end.column,
            endLineNumber: end.lineNumber,
            startColumn: start.column,
            startLineNumber: start.lineNumber,
        };
    }

    deltaDecorations(
        old: string[],
        next: Parameters<AnchorModel['deltaDecorations']>[1],
    ) {
        for (const id of old) {
            this.decorations.delete(id);
        }
        return next.map((d) => {
            const id = `d${this.nextId++}`;
            this.decorations.set(id, {
                end: this.getOffsetAt({
                    column: d.range.endColumn,
                    lineNumber: d.range.endLineNumber,
                }),
                start: this.getOffsetAt({
                    column: d.range.startColumn,
                    lineNumber: d.range.startLineNumber,
                }),
            });
            return id;
        });
    }

    /** Replace text at [start, end), moving decorations like Monaco does. */
    edit(start: number, end: number, insert: string) {
        const delta = insert.length - (end - start);
        this.text = this.text.slice(0, start) + insert + this.text.slice(end);
        for (const d of this.decorations.values()) {
            if (d.start >= end) {
                d.start += delta;
                d.end += delta;
            } else if (d.end > start) {
                d.end = Math.max(d.start, d.end + delta);
            }
        }
    }

    isDisposed() {
        return this.disposed;
    }

    get decorationCount() {
        return this.decorations.size;
    }
}

// V8 columns on line 1 carry the executor wrapper's 4-column indent.
const SOURCE = "$slider('vol', 1, 0, 2);\nconst t = $toggleBtn('t', true);";

function anchorsFor(model: FakeModel) {
    return createControlAnchors(model, [
        {
            fnName: '$slider',
            moduleId: 'S',
            sourceLocation: { column: 5, line: 1 },
        },
        {
            fnName: '$toggleBtn',
            moduleId: 'T',
            sourceLocation: { column: 11, line: 2 },
        },
    ]);
}

describe('ControlAnchors', () => {
    test('anchors each call at its callee name', () => {
        const model = new FakeModel(SOURCE);
        const anchors = anchorsFor(model);
        expect(anchors.offsetOf('S')).toBe(0);
        expect(anchors.offsetOf('T')).toBe(SOURCE.indexOf('$toggleBtn'));
        expect(anchors.offsetOf('missing')).toBeNull();
    });

    test('follows the call as the code before it is edited', () => {
        const model = new FakeModel(SOURCE);
        const anchors = anchorsFor(model);
        model.edit(0, 0, '// intro\n');
        expect(anchors.offsetOf('S')).toBe('// intro\n'.length);
        expect(anchors.offsetOf('T')).toBe(model.text.indexOf('$toggleBtn'));
    });

    test('renaming the label keeps the anchor', () => {
        const model = new FakeModel(SOURCE);
        const anchors = anchorsFor(model);
        const label = model.text.indexOf("'vol'");
        model.edit(label, label + 5, "'volume'");
        expect(anchors.offsetOf('S')).toBe(0);
    });

    test('anchors a group method call at its unprefixed name', () => {
        const model = new FakeModel(
            "const g = $cGroup('G');\ng.slider('x', 1, 0, 2);",
        );
        const anchors = createControlAnchors(model, [
            {
                fnName: '$slider',
                moduleId: 'S',
                sourceLocation: { column: 3, line: 2 },
            },
        ]);
        expect(anchors.offsetOf('S')).toBe(model.text.indexOf('slider('));
        // Renaming the group's label leaves the control anchored.
        const label = model.text.indexOf("'G'");
        model.edit(label, label + 3, "'Group'");
        expect(anchors.offsetOf('S')).toBe(model.text.indexOf('slider('));
    });

    test('editing the callee name drops the anchor', () => {
        const model = new FakeModel(SOURCE);
        const anchors = anchorsFor(model);
        model.edit(2, 4, '');
        expect(anchors.offsetOf('S')).toBeNull();
    });

    test('a call site the document no longer has gets no anchor', () => {
        const model = new FakeModel("$btn('x');");
        const anchors = createControlAnchors(model, [
            {
                fnName: '$slider',
                moduleId: 'S',
                sourceLocation: { column: 5, line: 1 },
            },
            {
                fnName: '$btn',
                moduleId: 'B',
                sourceLocation: { column: 1, line: 9 },
            },
            { fnName: '$btn', moduleId: 'C' },
        ]);
        expect(anchors.offsetOf('S')).toBeNull();
        expect(anchors.offsetOf('B')).toBeNull();
        expect(anchors.offsetOf('C')).toBeNull();
        expect(model.decorationCount).toBe(0);
    });

    test('dispose removes the decorations; a disposed model resolves nothing', () => {
        const model = new FakeModel(SOURCE);
        const anchors = anchorsFor(model);
        anchors.dispose();
        expect(model.decorationCount).toBe(0);

        const other = new FakeModel(SOURCE);
        const otherAnchors = anchorsFor(other);
        other.disposed = true;
        expect(otherAnchors.offsetOf('S')).toBeNull();
        otherAnchors.dispose();
    });
});
