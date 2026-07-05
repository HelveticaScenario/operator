use super::devices::{plan_deferrals, plan_prunes};
use super::parse::{MidiParseState, parse_midi_message};
use super::{MIDI_BUFFER_SIZE, MidiInputManager};
use modular_core::types::{DeviceName, Message};
use std::collections::{HashMap, HashSet};

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn defer_records_dropped_device() {
    let mut deferred = HashMap::new();
    let mut desired = HashSet::new();
    let to_open = plan_deferrals(
        &set(&["X"]),
        &set(&[]),
        false,
        2,
        &mut deferred,
        &mut desired,
    );
    assert_eq!(deferred.get("X"), Some(&2));
    assert!(desired.is_empty());
    assert!(to_open.is_empty());
}

#[test]
fn defer_keeps_earliest_id() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let mut desired = HashSet::new();
    plan_deferrals(
        &set(&["X"]),
        &set(&[]),
        false,
        3,
        &mut deferred,
        &mut desired,
    );
    assert_eq!(deferred.get("X"), Some(&2));
}

#[test]
fn rereference_cancels_defer() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let mut desired = HashSet::new();
    let to_open = plan_deferrals(
        &set(&["X"]),
        &set(&["X"]),
        false,
        3,
        &mut deferred,
        &mut desired,
    );
    assert!(deferred.is_empty());
    assert_eq!(desired, set(&["X"]));
    assert!(to_open.is_empty());
}

#[test]
fn new_device_scheduled_for_open() {
    let mut deferred = HashMap::new();
    let mut desired = HashSet::new();
    let to_open = plan_deferrals(
        &set(&[]),
        &set(&["Y"]),
        false,
        3,
        &mut deferred,
        &mut desired,
    );
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
    plan_deferrals(
        &set(&["X"]),
        &set(&["X"]),
        false,
        2,
        &mut deferred,
        &mut desired,
    );
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
    plan_deferrals(
        &set(&["X"]),
        &set(&[]),
        false,
        2,
        &mut deferred,
        &mut desired,
    );
    plan_deferrals(
        &set(&["X"]),
        &set(&[]),
        false,
        3,
        &mut deferred,
        &mut desired,
    );
    assert_eq!(deferred.get("X"), Some(&2));
    let to_close = plan_prunes(&mut deferred, &desired, 3);
    assert_eq!(to_close, vec!["X".to_string()]);
}

#[test]
fn wants_all_keeps_connected_devices() {
    // Explicit device → deviceless transition: a patch with a deviceless
    // MIDI module wants every open device kept, so no close is scheduled
    // and the device survives the subsequent prune.
    let mut deferred = HashMap::new();
    let mut desired = set(&["X"]);
    let to_open = plan_deferrals(
        &set(&["X"]),
        &set(&[]),
        true,
        2,
        &mut deferred,
        &mut desired,
    );
    assert!(deferred.is_empty());
    assert_eq!(desired, set(&["X"]));
    assert!(to_open.is_empty());
    let to_close = plan_prunes(&mut deferred, &desired, 9);
    assert!(to_close.is_empty());
}

#[test]
fn wants_all_cancels_pending_close_and_keeps_desired() {
    let mut deferred = HashMap::from([("X".to_string(), 2u64)]);
    let mut desired = set(&["Y"]);
    let to_open = plan_deferrals(
        &set(&["X"]),
        &set(&["Z"]),
        true,
        3,
        &mut deferred,
        &mut desired,
    );
    assert!(deferred.is_empty());
    // Explicitly connected devices are kept alongside the new ones.
    assert_eq!(desired, set(&["X", "Y", "Z"]));
    assert_eq!(to_open, vec!["Z".to_string()]);
}

#[test]
fn fourteen_bit_cc_never_grows_buffer() {
    let mut state = MidiParseState::new();
    let device = DeviceName::intern("test-device");

    // Store an MSB for CC 0 so a later LSB emits two messages.
    parse_midi_message(&[0xB0, 0x00, 0x40], &device, 0, &mut state);
    // Fill to one below capacity, mirroring the connection callback's
    // len-check admitting one more packet.
    while state.messages.len() < MIDI_BUFFER_SIZE - 1 {
        parse_midi_message(&[0xB0, 0x46, 0x01], &device, 0, &mut state);
    }
    // The LSB packet emits both the raw LSB CC and the combined 14-bit CC.
    parse_midi_message(&[0xB0, 0x20, 0x01], &device, 0, &mut state);

    assert_eq!(state.messages.len(), MIDI_BUFFER_SIZE);
    assert_eq!(state.messages.capacity(), MIDI_BUFFER_SIZE);
}

#[test]
fn prune_synthesizes_note_offs_for_held_notes() {
    let manager = MidiInputManager::new();
    let device = DeviceName::intern("Keystep");
    {
        let mut state = manager.parse_state.lock();
        parse_midi_message(&[0x90, 60, 100], &device, 10, &mut state);
        parse_midi_message(&[0x91, 64, 100], &device, 11, &mut state);
        parse_midi_message(&[0x80, 60, 0], &device, 12, &mut state);
    }
    manager
        .deferred_disconnects
        .lock()
        .insert("Keystep".to_string(), 2);

    manager.prune_disconnects(2);

    let state = manager.parse_state.lock();
    let offs: Vec<_> = state
        .messages
        .iter()
        .filter_map(|tm| match &tm.message {
            Message::MidiNoteOff(off) => Some((tm.timestamp_us, off.channel, off.note)),
            _ => None,
        })
        .collect();
    // The real note-off for note 60 plus the synthesized one for the note
    // still held (channel 1, note 64), timestamped no earlier than the
    // newest queued message so it sorts after the device's own traffic.
    assert_eq!(offs.len(), 2);
    assert!(offs.contains(&(12, 0, 60)));
    assert!(offs.contains(&(12, 1, 64)));
    assert!(state.held_notes.get("Keystep").is_none());
}
