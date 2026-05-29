// External store for the per-frame transport snapshot (BPM, bar/beat, Link
// phase, ...). The transport is polled ~60×/s; holding it in App-root React
// state re-rendered the entire App tree every frame. Routing it through an
// external store (useSyncExternalStore) means only the components that
// actually read it re-render — App itself does not.

import { useSyncExternalStore } from 'react';
import type { TransportSnapshot } from '../../shared/ipcTypes';

let snapshot: TransportSnapshot | null = null;
const listeners = new Set<() => void>();

function emit(): void {
    for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => {
        listeners.delete(listener);
    };
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
 * snapshot yet (matches the prior `prev ? {...} : prev` behaviour).
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
    return useSyncExternalStore(subscribe, () => snapshot);
}

/**
 * Just whether Ableton Link is enabled. Returns a primitive, so the caller
 * re-renders only when it flips — not on every per-frame snapshot update.
 */
export function useTransportLinkEnabled(): boolean {
    return useSyncExternalStore(
        subscribe,
        () => snapshot?.linkEnabled ?? false,
    );
}
