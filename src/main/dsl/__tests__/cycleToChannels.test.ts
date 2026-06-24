/**
 * Tests for the cycleToChannels() collection utility.
 */

import { beforeEach, describe, expect, test } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { DSLContext } from '../factories';
import {
    BaseCollection,
    type Collection,
    cycleToChannels,
    type GraphBuilder,
    type ModuleOutput,
} from '../GraphBuilder';

let builder: GraphBuilder;

beforeEach(() => {
    builder = new DSLContext(schemas).getBuilder();
});

/** An N-channel signal (one $sine per channel). */
function sig(channels: number): Collection {
    const freqs = Array.from({ length: channels }, (_, i) => 1 + i);
    return builder.getFactory('$sine')(freqs) as Collection;
}

/** Stable per-channel identifiers for a signal. */
function ids(s: Collection | ModuleOutput): string[] {
    const items = s instanceof BaseCollection ? [...s] : [s];
    return items.map((o) => o.toString());
}

describe('cycleToChannels()', () => {
    test('broadcasts a mono signal across N channels', () => {
        const mono = sig(1);
        const out = cycleToChannels(mono, 4) as Collection;
        expect(out.length).toBe(4);
        // every channel references the one source channel
        expect(ids(out)).toEqual(Array(4).fill(ids(mono)[0]));
    });

    test('cycles a wider signal (channel i ← source i % len)', () => {
        const stereo = sig(2);
        const [a, b] = ids(stereo);
        expect(ids(cycleToChannels(stereo, 5))).toEqual([a, b, a, b, a]);
    });

    test('truncates to a narrower target', () => {
        const out = cycleToChannels(sig(4), 2) as Collection;
        expect(out.length).toBe(2);
    });

    test('accepts a single ModuleOutput', () => {
        const single = [...sig(1)][0];
        expect((cycleToChannels(single, 3) as Collection).length).toBe(3);
    });
});
