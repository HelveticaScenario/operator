// These cover the store layer that backs the transport hooks. The hooks
// themselves (useTransport / useTransportLinkEnabled) are thin
// useSyncExternalStore wrappers over the subscribe + getSnapshot functions
// exercised here; the suite runs in a node environment with no DOM renderer,
// so the hooks are not rendered directly.

import { afterEach, describe, expect, it, vi } from 'vitest';
import type { TransportSnapshot } from '../../../shared/ipcTypes';
import {
    getLinkEnabledSnapshot,
    getTransportSnapshot,
    setTransport,
    subscribeTransport,
    updateTransport,
} from '../transportStore';

function makeSnapshot(
    overrides: Partial<TransportSnapshot> = {},
): TransportSnapshot {
    return {
        barPhase: 0,
        bar: 0,
        beatInBar: 0,
        bpm: 120,
        timeSigNumerator: 4,
        timeSigDenominator: 4,
        isPlaying: false,
        hasQueuedUpdate: false,
        lastAppliedUpdateId: 0,
        linkEnabled: false,
        linkPeers: 0,
        linkPhase: 0,
        linkPendingStart: false,
        ...overrides,
    };
}

// The store is a module singleton; reset it to null between tests via the
// public API so cases don't leak state into one another.
afterEach(() => {
    setTransport(null);
});

describe('transportStore', () => {
    it('starts with no snapshot', () => {
        expect(getTransportSnapshot()).toBeNull();
        expect(getLinkEnabledSnapshot()).toBe(false);
    });

    it('setTransport notifies subscribers and stores the snapshot by reference', () => {
        const listener = vi.fn();
        const unsubscribe = subscribeTransport(listener);

        const snap = makeSnapshot({ bpm: 90 });
        setTransport(snap);

        expect(listener).toHaveBeenCalledTimes(1);
        // Stored by reference, not copied: useSyncExternalStore needs getSnapshot
        // to return the same value when nothing changed, so the store must not
        // allocate a fresh object on read.
        expect(getTransportSnapshot()).toBe(snap);
        unsubscribe();
    });

    it('updateTransport patches the snapshot and notifies', () => {
        setTransport(makeSnapshot({ linkEnabled: false, linkPeers: 3 }));
        const listener = vi.fn();
        const unsubscribe = subscribeTransport(listener);

        updateTransport((prev) => ({
            ...prev,
            linkEnabled: true,
            linkPeers: prev.linkPeers,
        }));

        expect(listener).toHaveBeenCalledTimes(1);
        expect(getTransportSnapshot()?.linkEnabled).toBe(true);
        expect(getTransportSnapshot()?.linkPeers).toBe(3);
        unsubscribe();
    });

    it('updateTransport is a no-op (and does not notify) with no snapshot', () => {
        const listener = vi.fn();
        const unsubscribe = subscribeTransport(listener);

        updateTransport((prev) => ({ ...prev, linkEnabled: true }));

        expect(listener).not.toHaveBeenCalled();
        expect(getTransportSnapshot()).toBeNull();
        unsubscribe();
    });

    it('getLinkEnabledSnapshot derives the primitive from the snapshot', () => {
        setTransport(makeSnapshot({ linkEnabled: true }));
        expect(getLinkEnabledSnapshot()).toBe(true);
        setTransport(makeSnapshot({ linkEnabled: false }));
        expect(getLinkEnabledSnapshot()).toBe(false);
    });

    it('keeps the link primitive Object.is-stable while snapshots churn', () => {
        // Crux of the perf fix. React reads getSnapshot after every
        // notification and re-renders only when Object.is reports a change.
        // Sample both reads inside the subscriber — the notification is React's
        // re-evaluation point — so this mirrors what the reconciler sees: each
        // frame pushes a fresh snapshot object, so useTransport churns
        // (re-render, by design), but linkEnabled is unchanged, so
        // useTransportLinkEnabled dedups and App skips the re-render.
        const snapshotReads: (TransportSnapshot | null)[] = [];
        const linkReads: boolean[] = [];
        const unsubscribe = subscribeTransport(() => {
            snapshotReads.push(getTransportSnapshot());
            linkReads.push(getLinkEnabledSnapshot());
        });

        setTransport(makeSnapshot({ linkEnabled: true, barPhase: 0.1 }));
        setTransport(makeSnapshot({ linkEnabled: true, barPhase: 0.2 }));

        expect(snapshotReads[0]).not.toBe(snapshotReads[1]); // ref churns
        expect(Object.is(linkReads[0], linkReads[1])).toBe(true); // primitive does not
        unsubscribe();
    });

    it('notifies all subscribers and stops after unsubscribe', () => {
        const a = vi.fn();
        const b = vi.fn();
        const unsubA = subscribeTransport(a);
        const unsubB = subscribeTransport(b);

        setTransport(makeSnapshot());
        expect(a).toHaveBeenCalledTimes(1);
        expect(b).toHaveBeenCalledTimes(1);

        unsubA();
        setTransport(makeSnapshot());
        expect(a).toHaveBeenCalledTimes(1); // no further calls
        expect(b).toHaveBeenCalledTimes(2);
        unsubB();
    });
});
