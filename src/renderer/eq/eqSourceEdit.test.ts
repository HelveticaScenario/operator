import { describe, expect, it } from 'vitest';

import type { CodeStyle } from '../dsl/objectPropertyInsert';
import {
    computeEqChannelEdit,
    computeEqSourceEdit,
    type EqChannelOp,
    type EqListTarget,
    type EqSourceOp,
} from './eqSourceEdit';

const layout: CodeStyle = {
    bracketSpacing: true,
    printWidth: 80,
    quote: "'",
    tabWidth: 4,
    trailingComma: true,
    useTabs: false,
};

const str = (text: string) => (q: string) => `${q}${text}${q}`;
const num = (text: string) => () => text;

const shared = (count: number) => ({ count, list: null, listCount: 1 });

function run(
    source: string,
    count: number | EqListTarget,
    op: EqSourceOp,
): string {
    const anchor = source.lastIndexOf('eq(');
    const target = typeof count === 'number' ? shared(count) : count;
    const res = computeEqSourceEdit(source, anchor, target, op, layout);
    if (!res.ok) {
        throw new Error(res.reason);
    }
    if (!res.edit) {
        return source;
    }
    return (
        source.slice(0, res.edit.start) +
        res.edit.text +
        source.slice(res.edit.end)
    );
}

function result(source: string, target: EqListTarget, op: EqSourceOp) {
    return computeEqSourceEdit(
        source,
        source.lastIndexOf('eq('),
        target,
        op,
        layout,
    );
}

const multi = `const x = $unstable.eq($saw('c2'), {
    sections: [
        { type: 'lowShelf', freq: "150hz", gain: 3 },
        { freq: '1000hz', gain: -6, q: 2 }, // mids
    ],
    ui: true,
}).out();
`;

describe('computeEqSourceEdit set', () => {
    it('rewrites existing literals, keeping each string literal quote', () => {
        const out = run(multi, 2, {
            fields: { freq: str('180hz'), gain: num('4.5') },
            index: 0,
            kind: 'set',
        });
        expect(out).toContain(`{ type: 'lowShelf', freq: "180hz", gain: 4.5 }`);
    });

    it('rewrites negative numbers', () => {
        const out = run(multi, 2, {
            fields: { gain: num('-2') },
            index: 1,
            kind: 'set',
        });
        expect(out).toContain(`{ freq: '1000hz', gain: -2, q: 2 }, // mids`);
    });

    it('inserts absent properties', () => {
        const out = run(multi, 2, {
            fields: { q: num('0.5'), type: str('peak') },
            index: 0,
            kind: 'set',
        });
        expect(out).toContain(
            `{ type: 'peak', freq: "150hz", gain: 3, q: 0.5 }`,
        );
    });

    it('skips non-literal values and still writes the rest', () => {
        const src = `$unstable.eq(x, { sections: [{ freq: lfo, gain: g }], ui: true })`;
        const op: EqSourceOp = {
            fields: { freq: str('100hz'), q: num('3') },
            index: 0,
            kind: 'set',
        };
        const res = result(src, shared(1), op);
        expect(res.ok && res.skipped).toEqual(['freq']);
        expect(run(src, 1, op)).toBe(
            `$unstable.eq(x, { sections: [{ freq: lfo, gain: g, q: 3 }], ui: true })`,
        );
    });

    it('reports a hand-changed section count as stale', () => {
        expect(
            result(multi, shared(3), {
                fields: { gain: num('1') },
                index: 0,
                kind: 'set',
            }),
        ).toEqual({ ok: false, reason: 'stale' });
    });

    it('reports a moved anchor as stale', () => {
        const res = computeEqSourceEdit(
            multi,
            multi.indexOf('eq(') + 1,
            shared(2),
            { fields: { gain: num('1') }, index: 0, kind: 'set' },
            layout,
        );
        expect(res).toEqual({ ok: false, reason: 'stale' });
    });

    it('reports a non-literal sections value as unsupported', () => {
        expect(
            result(
                `$unstable.eq(x, { sections: bands, ui: true })`,
                shared(1),
                {
                    fields: { gain: num('1') },
                    index: 0,
                    kind: 'set',
                },
            ),
        ).toEqual({ ok: false, reason: 'unsupported' });
    });

    it('ignores other eq calls and strings spelling eq(', () => {
        const src = `a.eq(1); const s = 'eq('; $unstable.eq(x, { sections: [{ freq: 0 }], ui: true })`;
        expect(
            run(src, 1, { fields: { freq: num('1') }, index: 0, kind: 'set' }),
        ).toBe(
            `a.eq(1); const s = 'eq('; $unstable.eq(x, { sections: [{ freq: 1 }], ui: true })`,
        );
    });
});

describe('computeEqSourceEdit per-channel lists', () => {
    const perChannel = `$unstable.eq(x, {
    sections: [
        [{ freq: 0 }],
        [{ freq: 1 }, { freq: 2 }],
    ],
    ui: true,
})`;

    it('edits only the addressed channel list', () => {
        const second = { count: 2, list: 1, listCount: 2 };
        expect(
            run(perChannel, second, {
                fields: { gain: num('3') },
                index: 1,
                kind: 'set',
            }),
        ).toContain(`[{ freq: 1 }, { freq: 2, gain: 3 }],`);
        expect(
            run(
                perChannel,
                { count: 1, list: 0, listCount: 2 },
                { fields: { freq: num('5') }, kind: 'add' },
            ),
        ).toContain(`[{ freq: 0 }, { freq: 5 }],`);
        expect(run(perChannel, second, { index: 0, kind: 'remove' })).toContain(
            `[{ freq: 2 }],`,
        );
    });

    it('reports a changed form or channel count as stale', () => {
        const op: EqSourceOp = {
            fields: { freq: num('1') },
            index: 0,
            kind: 'set',
        };
        expect(result(perChannel, shared(2), op)).toEqual({
            ok: false,
            reason: 'stale',
        });
        expect(
            result(perChannel, { count: 1, list: 0, listCount: 3 }, op),
        ).toEqual({ ok: false, reason: 'stale' });
    });

    it('reports a non-literal channel list as unsupported', () => {
        expect(
            result(
                `$unstable.eq(x, { sections: [[{ freq: 0 }], bands], ui: true })`,
                { count: 1, list: 1, listCount: 2 },
                { fields: { freq: num('1') }, index: 0, kind: 'set' },
            ),
        ).toEqual({ ok: false, reason: 'unsupported' });
    });
});

describe('computeEqSourceEdit call forms', () => {
    it('finds sections in the $ chain form', () => {
        const src = `$saw('c2').$.unstable.eq({ sections: [{ freq: 0 }], ui: true })`;
        expect(
            run(src, 1, { fields: { gain: num('2') }, index: 0, kind: 'set' }),
        ).toBe(
            `$saw('c2').$.unstable.eq({ sections: [{ freq: 0, gain: 2 }], ui: true })`,
        );
    });

    it('needs the config holding ui', () => {
        expect(
            result(`$unstable.eq(x, { sections: [{ freq: 0 }] })`, shared(1), {
                fields: { gain: num('1') },
                index: 0,
                kind: 'set',
            }),
        ).toEqual({ ok: false, reason: 'unsupported' });
    });
});

describe('computeEqSourceEdit add/remove', () => {
    it('adds sections to a config without them', () => {
        expect(
            run(`$unstable.eq(x, { ui: true })`, 0, {
                fields: { freq: str('1000hz'), type: str('peak') },
                kind: 'add',
            }),
        ).toBe(
            `$unstable.eq(x, { ui: true, sections: [{ type: 'peak', freq: '1000hz' }] })`,
        );
        // Only an empty shared list can be missing.
        expect(
            result(`$unstable.eq(x, { ui: true })`, shared(1), {
                fields: { freq: num('1') },
                kind: 'add',
            }),
        ).toEqual({ ok: false, reason: 'stale' });
    });

    it('appends to an empty array', () => {
        expect(
            run(`$unstable.eq(x, { sections: [], ui: true })`, 0, {
                fields: { freq: num('1') },
                kind: 'add',
            }),
        ).toBe(`$unstable.eq(x, { sections: [{ freq: 1 }], ui: true })`);
    });

    it('appends to an expanded array on its own line', () => {
        const out = run(multi, 2, {
            fields: { freq: str('5000hz'), gain: num('2'), type: str('peak') },
            kind: 'add',
        });
        expect(out).toContain(
            `        { freq: '1000hz', gain: -6, q: 2 }, // mids
        { type: 'peak', freq: '5000hz', gain: 2 },
    ],`,
        );
    });

    it('appends to an expanded array without a trailing comma', () => {
        const src = `$unstable.eq(x, { ui: true, sections: [
  { freq: 0 }
] })`;
        expect(run(src, 1, { fields: { freq: num('1') }, kind: 'add' }))
            .toBe(`$unstable.eq(x, { ui: true, sections: [
  { freq: 0 },
  { freq: 1 }
] })`);
    });

    it('appends inline to a one-line array', () => {
        expect(
            run(`$unstable.eq(x, { sections: [{ freq: 0 }], ui: true })`, 1, {
                fields: { freq: num('1') },
                kind: 'add',
            }),
        ).toBe(
            `$unstable.eq(x, { sections: [{ freq: 0 }, { freq: 1 }], ui: true })`,
        );
    });

    it('removes a first or middle element with its separator', () => {
        expect(run(multi, 2, { index: 0, kind: 'remove' })).toContain(
            `    sections: [
        { freq: '1000hz', gain: -6, q: 2 }, // mids
    ],`,
        );
        expect(
            run(
                `$unstable.eq(x, { sections: [{ freq: 0 }, { freq: 1 }, { freq: 2 }], ui: true })`,
                3,
                { index: 1, kind: 'remove' },
            ),
        ).toBe(
            `$unstable.eq(x, { sections: [{ freq: 0 }, { freq: 2 }], ui: true })`,
        );
    });

    it('removes the last element, keeping a trailing comma', () => {
        expect(run(multi, 2, { index: 1, kind: 'remove' })).toContain(
            `    sections: [
        { type: 'lowShelf', freq: "150hz", gain: 3 }, // mids
    ],`,
        );
    });

    it('removing the only section leaves an empty list', () => {
        expect(
            run(`$unstable.eq(x, { sections: [{ freq: 0 }], ui: true })`, 1, {
                index: 0,
                kind: 'remove',
            }),
        ).toBe(`$unstable.eq(x, { sections: [], ui: true })`);
    });
});

describe('computeEqChannelEdit', () => {
    const channel = (
        source: string,
        listCount: number,
        op: EqChannelOp,
    ): string => {
        const res = computeEqChannelEdit(
            source,
            source.lastIndexOf('eq('),
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
            channel(
                `$unstable.eq(x, { sections: [{ freq: 0 }], ui: true })`,
                0,
                {
                    kind: 'split',
                },
            ),
        ).toBe(
            `$unstable.eq(x, { sections: [[{ freq: 0 }], [{ freq: 0 }]], ui: true })`,
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
        expect(
            channel(`$unstable.eq(x, { ui: true })`, 0, { kind: 'split' }),
        ).toBe(`$unstable.eq(x, { ui: true, sections: [[], []] })`);
    });

    it('adds a copy of a channel list', () => {
        expect(
            channel(
                `$unstable.eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }]], ui: true })`,
                2,
                { from: 1, kind: 'addChannel' },
            ),
        ).toBe(
            `$unstable.eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }], [{ freq: 1 }]], ui: true })`,
        );
    });

    it('removes a channel list, collapsing the last two to the shared form', () => {
        const three = `$unstable.eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }], [{ freq: 2 }]], ui: true })`;
        expect(channel(three, 3, { index: 1, kind: 'removeChannel' })).toBe(
            `$unstable.eq(x, { sections: [[{ freq: 0 }], [{ freq: 2 }]], ui: true })`,
        );
        const two = `$unstable.eq(x, {
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
            `$unstable.eq(x, {
    sections: [
        { freq: 0 },
        { freq: 1 },
    ],
    ui: true,
})`,
        );
    });

    it('reports a mismatched form or channel count as stale', () => {
        const per = `$unstable.eq(x, { sections: [[{ freq: 0 }], [{ freq: 1 }]], ui: true })`;
        const anchor = per.lastIndexOf('eq(');
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
