import { describe, expect, it } from 'vitest';

import type { CodeStyle } from '../dsl/objectPropertyInsert';
import { computeEqChannelEdit, type EqChannelOp } from './eqChannelEdit';

const layout: CodeStyle = {
    bracketSpacing: true,
    printWidth: 80,
    quote: "'",
    tabWidth: 4,
    trailingComma: true,
    useTabs: false,
};

/** The panel's anchor: the `$eq` callee, or the `eq` of a `.$.eq(` chain. */
const anchorOf = (source: string) =>
    source.includes('.$.eq(')
        ? source.lastIndexOf('eq(')
        : source.lastIndexOf('$eq(');

const multi = `const x = $eq($saw('c2'), {
    sections: [
        { type: 'lowShelf', freq: "150hz", gain: 3 },
        { freq: '1000hz', gain: -6, q: 2 }, // mids
    ],
    ui: true,
}).out();
`;

describe('computeEqChannelEdit', () => {
    const channel = (
        source: string,
        listCount: number,
        op: EqChannelOp,
    ): string => {
        const res = computeEqChannelEdit(
            source,
            anchorOf(source),
            listCount,
            op,
            layout,
        );
        if (!res.ok) {
            throw new Error(res.reason);
        }
        return res.edit
            ? source.slice(0, res.edit.start) +
                  res.edit.text +
                  source.slice(res.edit.end)
            : source;
    };

    it('splits a one-line shared list into two copies inline', () => {
        expect(
            channel(`$eq(x, { sections: [{ freq: 0 }], ui: true })`, 0, {
                kind: 'split',
            }),
        ).toBe(
            `$eq(x, { sections: [[{ freq: 0 }], [{ freq: 0 }]], ui: true })`,
        );
    });

    it('splits a multi-line shared list one copy per line', () => {
        expect(channel(multi, 0, { kind: 'split' })).toContain(
            `    sections: [
        [
            { type: 'lowShelf', freq: "150hz", gain: 3 },
            { freq: '1000hz', gain: -6, q: 2 }, // mids
        ],
        [
            { type: 'lowShelf', freq: "150hz", gain: 3 },
            { freq: '1000hz', gain: -6, q: 2 }, // mids
        ],
    ],`,
        );
    });

    it('splits a config without sections into two empty lists', () => {
        expect(channel(`$eq(x, { ui: true })`, 0, { kind: 'split' })).toBe(
            `$eq(x, { ui: true, sections: [[], []] })`,
        );
    });

    it('adds a copy of a channel list', () => {
        expect(
            channel(
                `$eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }]], ui: true })`,
                2,
                { from: 1, kind: 'addChannel' },
            ),
        ).toBe(
            `$eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }], [{ freq: 1 }]], ui: true })`,
        );
    });

    it('removes a channel list, collapsing the last two to the shared form', () => {
        const three = `$eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }], [{ freq: 2 }]], ui: true })`;
        expect(channel(three, 3, { index: 1, kind: 'removeChannel' })).toBe(
            `$eq(x, { sections: [[{ freq: 0 }], [{ freq: 2 }]], ui: true })`,
        );
        const two = `$eq(x, {
    sections: [
        [
            { freq: 0 },
            { freq: 1 },
        ],
        [{ freq: 2 }],
    ],
    ui: true,
})`;
        expect(channel(two, 2, { index: 1, kind: 'removeChannel' })).toBe(
            `$eq(x, {
    sections: [
        { freq: 0 },
        { freq: 1 },
    ],
    ui: true,
})`,
        );
    });

    it('reports a mismatched form or channel count as stale', () => {
        const per = `$eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }]], ui: true })`;
        const anchor = anchorOf(per);
        expect(
            computeEqChannelEdit(per, anchor, 0, { kind: 'split' }, layout),
        ).toEqual({ ok: false, reason: 'stale' });
        expect(
            computeEqChannelEdit(
                per,
                anchor,
                3,
                { index: 0, kind: 'removeChannel' },
                layout,
            ),
        ).toEqual({ ok: false, reason: 'stale' });
    });
});
