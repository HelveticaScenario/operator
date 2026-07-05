use super::devices::{plan_deferrals, plan_prunes};
use std::collections::{HashMap, HashSet};

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn defer_records_dropped_device() {
    let mut deferred = HashMap::new();
    let mut desired = HashSet::new();
    let to_open = plan_deferrals(&set(&["X"]), &set(&[]), 2, &mut deferred, &mut desired);
    assert_eq!(deferred.get("X"), Some(&2));
    assert!(desired.is_empty());
    assert!(to_open.is_empty());
}

#[test]
fn defer_keeps_earliest_id() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let mut desired = HashSet::new();
    plan_deferrals(&set(&["X"]), &set(&[]), 3, &mut deferred, &mut desired);
    assert_eq!(deferred.get("X"), Some(&2));
}

#[test]
fn rereference_cancels_defer() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let mut desired = HashSet::new();
    let to_open = plan_deferrals(&set(&["X"]), &set(&["X"]), 3, &mut deferred, &mut desired);
    assert!(deferred.is_empty());
    assert_eq!(desired, set(&["X"]));
    assert!(to_open.is_empty());
}

#[test]
fn new_device_scheduled_for_open() {
    let mut deferred = HashMap::new();
    let mut desired = HashSet::new();
    let to_open = plan_deferrals(&set(&[]), &set(&["Y"]), 3, &mut deferred, &mut desired);
    assert_eq!(to_open, vec!["Y".to_string()]);
    assert_eq!(desired, set(&["Y"]));
    assert!(deferred.is_empty());
}

#[test]
fn multi_module_same_device_not_deferred() {
    // sync_midi_devices_from_patch dedups to a set, so a device used by two
    // modules stays present when only one module is removed.
    let mut deferred = HashMap::new();
    let mut desired = HashSet::new();
    plan_deferrals(&set(&["X"]), &set(&["X"]), 2, &mut deferred, &mut desired);
    assert!(deferred.is_empty());
}

#[test]
fn prune_closes_when_applied_ge_uid() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let to_close = plan_prunes(&mut deferred, &set(&[]), 2);
    assert_eq!(to_close, vec!["X".to_string()]);
    assert!(deferred.is_empty());
}

#[test]
fn prune_skips_when_not_yet_applied() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let to_close = plan_prunes(&mut deferred, &set(&[]), 1);
    assert!(to_close.is_empty());
    assert_eq!(deferred.get("X"), Some(&2));
}

#[test]
fn prune_skips_when_still_desired() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let to_close = plan_prunes(&mut deferred, &set(&["X"]), 9);
    assert!(to_close.is_empty());
    assert_eq!(deferred.get("X"), Some(&2));
}

#[test]
fn prune_selects_subset() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64), ("Y".to_string(), 4u64)]);
    let to_close = plan_prunes(&mut deferred, &set(&[]), 3);
    assert_eq!(to_close, vec!["X".to_string()]);
    assert_eq!(deferred.get("Y"), Some(&4));
}

#[test]
fn no_prune_before_first_apply() {
    let mut deferred = HashMap::from([("X".to_string(), 1u64)]);
    let to_close = plan_prunes(&mut deferred, &set(&[]), 0);
    assert!(to_close.is_empty());
}

#[test]
fn supersede_then_prune() {
    // B(id 2) and C(id 3) both drop X; or_insert keeps {X:2}. C applies → 3.
    let mut deferred = HashMap::new();
    let mut desired = HashSet::new();
    plan_deferrals(&set(&["X"]), &set(&[]), 2, &mut deferred, &mut desired);
    plan_deferrals(&set(&["X"]), &set(&[]), 3, &mut deferred, &mut desired);
    assert_eq!(deferred.get("X"), Some(&2));
    let to_close = plan_prunes(&mut deferred, &desired, 3);
    assert_eq!(to_close, vec!["X".to_string()]);
}
