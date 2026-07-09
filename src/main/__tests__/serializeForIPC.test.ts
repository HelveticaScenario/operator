import { describe, expect, test } from 'vitest';
import { serializeForIPC } from '../serializeForIPC';

describe('serializeForIPC', () => {
    test('an object shared by two siblings serializes fully both times', () => {
        const cfg = { theme: 'dark' };
        expect(serializeForIPC({ after: cfg, before: cfg })).toEqual({
            after: { theme: 'dark' },
            before: { theme: 'dark' },
        });
    });

    test('an array holding the same element twice serializes both entries', () => {
        const entry = { id: 1 };
        expect(serializeForIPC([entry, entry])).toEqual([{ id: 1 }, { id: 1 }]);
    });

    test('a value repeated at different depths is not flagged as circular', () => {
        const leaf = { n: 1 };
        expect(serializeForIPC({ a: { leaf }, b: leaf })).toEqual({
            a: { leaf: { n: 1 } },
            b: { n: 1 },
        });
    });

    test('a true cycle is reported as [Circular]', () => {
        const obj: Record<string, unknown> = { name: 'root' };
        obj.self = obj;
        expect(serializeForIPC(obj)).toEqual({
            name: 'root',
            self: '[Circular]',
        });
    });

    test('a cycle through an array is reported as [Circular]', () => {
        const arr: unknown[] = [1];
        arr.push(arr);
        expect(serializeForIPC(arr)).toEqual([1, '[Circular]']);
    });

    test('deeply nested sharing hits the node budget instead of expanding exponentially', () => {
        // A 32-level diamond has ~2^33 paths; without a budget this never
        // finishes. The budget caps the work and marks the cut-off points.
        let node: Record<string, unknown> = { v: 0 };
        for (let i = 0; i < 32; i++) {
            node = { a: node, b: node };
        }
        const start = performance.now();
        const result = serializeForIPC(node);
        expect(performance.now() - start).toBeLessThan(1000);
        expect(JSON.stringify(result)).toContain('"[Truncated]"');
    });

    test('Maps and Sets with shared values serialize fully', () => {
        const shared = { x: 1 };
        const map = new Map<string, unknown>([
            ['a', shared],
            ['b', shared],
        ]);
        expect(serializeForIPC(map)).toEqual({
            __type: 'Map',
            entries: [
                ['a', { x: 1 }],
                ['b', { x: 1 }],
            ],
        });
        expect(serializeForIPC([new Set([shared]), shared])).toEqual([
            { __type: 'Set', values: [{ x: 1 }] },
            { x: 1 },
        ]);
    });

    test('Map entries keep distinct keys that share a string form', () => {
        const map = new Map<unknown, number>([
            [1, 10],
            ['1', 20],
        ]);
        expect(serializeForIPC(map)).toEqual({
            __type: 'Map',
            entries: [
                [1, 10],
                ['1', 20],
            ],
        });
    });

    test("an own '__proto__' key serializes as a plain property", () => {
        const parsed = JSON.parse('{"__proto__": {"x": 1}, "y": 2}');
        const result = serializeForIPC(parsed) as Record<string, unknown>;
        expect(Object.keys(result)).toEqual(['__proto__', 'y']);
        expect(result['__proto__']).toEqual({ x: 1 });
        expect(result.y).toBe(2);
    });

    test('built-ins from another realm are recognized', async () => {
        const vm = await import('node:vm');
        const foreign = vm.runInNewContext(
            `({
                err: new Error('boom'),
                date: new Date(0),
                map: new Map([['k', 1]]),
                set: new Set([1]),
            })`,
        );
        expect(serializeForIPC(foreign)).toEqual({
            err: expect.objectContaining({ __error: true, message: 'boom' }),
            date: '1970-01-01T00:00:00.000Z',
            map: { __type: 'Map', entries: [['k', 1]] },
            set: { __type: 'Set', values: [1] },
        });
    });
});
