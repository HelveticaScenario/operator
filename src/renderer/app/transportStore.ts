// External store for the per-frame transport snapshot (BPM, bar/beat, Link
// phase, ...). The transport is polled ~60×/s; an external store
// (useSyncExternalStore) confines those updates to the components that read
// the snapshot, so the App tree does not re-render on every frame.

import { useSyncExternalStore } from 'react';
import type { TransportSnapshot } from '../../shared/ipcTypes';

let snapshot: TransportSnapshot | null = null;
const listeners = new Set<() => void>();

function emit(): void {
    for (const listener of listeners) listener();
}

/** Register a listener; returns an unsubscribe function. */
export function subscribeTransport(listener: () => void): () => void {
    listeners.add(listener);
    return () => {
        listeners.delete(listener);
    };
}

/** Current transport snapshot, or null before the first poll. */
export function getTransportSnapshot(): TransportSnapshot | null {
    return snapshot;
}

/** Whether Ableton Link is enabled; a primitive derived from the snapshot. */
export function getLinkEnabledSnapshot(): boolean {
    return snapshot?.linkEnabled ?? false;
}

/**
 * Replace the current transport snapshot and notify subscribers. Called from
 * App's per-frame poll loop; re-renders only the subscribers (the transport
 * display), not the App tree.
 */
export function setTransport(next: TransportSnapshot | null): void {
    snapshot = next;
    emit();
}

/**
 * Merge an optimistic update into the current snapshot. No-op when there is no
 * snapshot yet.
 */
export function updateTransport(
    patch: (prev: TransportSnapshot) => TransportSnapshot,
): void {
    if (snapshot === null) return;
    snapshot = patch(snapshot);
    emit();
}

/** Full snapshot; re-renders the caller on every transport change. */
export function useTransport(): TransportSnapshot | null {
    return useSyncExternalStore(subscribeTransport, getTransportSnapshot);
}

/**
 * Just whether Ableton Link is enabled. Returns a primitive, so the caller
 * re-renders only when it flips — not on every per-frame snapshot update.
 */
export function useTransportLinkEnabled(): boolean {
    return useSyncExternalStore(subscribeTransport, getLinkEnabledSnapshot);
}
