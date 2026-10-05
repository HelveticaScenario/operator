//! Pure device-lifecycle planning for the manager's patch-update sync and
//! deferred-close pruning. Kept free of MIDI I/O so it is unit-testable
//! without real ports.

use std::collections::{HashMap, HashSet};

/// Pure bookkeeping for `sync_devices`: record deferred closes for dropped
/// devices, cancel deferrals for re-referenced ones, set `desired`, and return
/// the devices to open (wanted − currently connected).
pub(super) fn plan_deferrals(
    connected: &HashSet<String>,
    device_names: &HashSet<String>,
    wants_all_devices: bool,
    update_id: u64,
    deferred: &mut HashMap<String, u64>,
    desired: &mut HashSet<String>,
) -> Vec<String> {
    if wants_all_devices {
        // A deviceless MIDI module receives from every open device, so every
        // connected (and previously desired) device stays wanted: cancel all
        // pending closes and keep `desired` a superset of what it was.
        deferred.clear();
        desired.extend(device_names.iter().cloned());
        desired.extend(connected.iter().cloned());
        return device_names.difference(connected).cloned().collect();
    }

    // Open devices that are no longer wanted → schedule a deferred close, keeping
    // the earliest id (soonest provably-safe close; a still-pending entry implies
    // no re-reference has happened since, so every later update also drops it).
    for name in connected {
        if !device_names.contains(name) {
            deferred.entry(name.clone()).or_insert(update_id);
        }
    }
    // Wanted devices → cancel any pending close.
    for name in device_names {
        deferred.remove(name);
    }
    *desired = device_names.clone();
    device_names.difference(connected).cloned().collect()
}

/// Pure bookkeeping for `prune_disconnects`: drain and return deferred devices
/// whose dropping update has applied (`uid <= applied`) and that the current
/// patch does not want. The `desired` guard covers the explicit `connect()`
/// path, which re-adds a device without going through `plan_deferrals`.
pub(super) fn plan_prunes(
    deferred: &mut HashMap<String, u64>,
    desired: &HashSet<String>,
    applied: u64,
) -> Vec<String> {
    let to_close: Vec<String> = deferred
        .iter()
        .filter(|(name, uid)| **uid <= applied && !desired.contains(*name))
        .map(|(name, _)| name.clone())
        .collect();
    for name in &to_close {
        deferred.remove(name);
    }
    to_close
}
