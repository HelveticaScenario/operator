//! MIDI input handling for the modular synthesizer.
//!
//! This module provides MIDI device enumeration, connection management,
//! and converts raw MIDI bytes to Message types for dispatch to DSP modules.
//! Supports multiple simultaneous MIDI device connections.
//! Messages are timestamped and sorted to ensure correct ordering.
//! Supports 14-bit CC messages (CC 0-31 MSB + CC 32-63 LSB).

use midir::{MidiInput, MidiInputConnection};
use modular_core::types::{
  Message, MidiChannelPressure, MidiControlChange, MidiControlChange14Bit, MidiNoteOff, MidiNoteOn,
  MidiPitchBend, MidiPolyPressure,
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Maximum MIDI messages buffered
const MIDI_BUFFER_SIZE: usize = 1024;

/// A MIDI message with its timestamp (microseconds from midir)
#[derive(Debug, Clone)]
struct TimestampedMessage {
  /// Timestamp in microseconds (from midir callback)
  timestamp_us: u64,
  /// The parsed MIDI message
  message: Message,
}

/// State for tracking 14-bit CC MSB values per device
/// Key: (device_name, channel, cc_msb), Value: msb_value
#[derive(Debug, Default)]
struct MidiCcState {
  /// MSB values waiting for LSB: [channel][cc] -> msb_value
  /// Only CC 0-31 can have MSB (their LSB is CC 32-63)
  msb_values: [[Option<u8>; 32]; 16],
}

impl MidiCcState {
  fn new() -> Self {
    Self {
      msb_values: [[None; 32]; 16],
    }
  }

  /// Store MSB value for later combination with LSB
  fn set_msb(&mut self, channel: u8, cc: u8, value: u8) {
    if cc < 32 && channel < 16 {
      self.msb_values[channel as usize][cc as usize] = Some(value);
    }
  }

  /// Take the stored MSB value for a given channel/cc, if any
  fn take_msb(&mut self, channel: u8, cc_msb: u8) -> Option<u8> {
    if cc_msb < 32 && channel < 16 {
      self.msb_values[channel as usize][cc_msb as usize].take()
    } else {
      None
    }
  }
}

/// Information about a MIDI input port
#[derive(Debug, Clone)]
pub struct MidiPortInfo {
  pub name: String,
  pub index: usize,
}

/// Shared state for MIDI parsing across callbacks
struct MidiParseState {
  /// Pending messages with timestamps
  messages: Vec<TimestampedMessage>,
  /// 14-bit CC state per device
  cc_state: HashMap<String, MidiCcState>,
}

impl MidiParseState {
  fn new() -> Self {
    Self {
      messages: Vec::with_capacity(MIDI_BUFFER_SIZE),
      cc_state: HashMap::new(),
    }
  }

  fn get_cc_state(&mut self, device: &str) -> &mut MidiCcState {
    self
      .cc_state
      .entry(device.to_string())
      .or_insert_with(MidiCcState::new)
  }
}

/// Manages multiple MIDI input connections
pub struct MidiInputManager {
  /// Active connections keyed by device name
  connections: Mutex<HashMap<String, MidiInputConnection<()>>>,
  /// Shared parse state (messages + 14-bit CC tracking)
  parse_state: Arc<Mutex<MidiParseState>>,
  /// Device names we want to be connected to (for reconnection)
  desired_devices: Mutex<HashSet<String>>,
  /// Devices a patch update stopped referencing, mapped to the update_id whose
  /// application makes the close safe. Closing is deferred until that update is
  /// applied on the audio thread so the still-playing patch keeps the device
  /// until its swap. Pruned by `prune_disconnects`. (Opens are eager; only
  /// closes are deferred.)
  deferred_disconnects: Mutex<HashMap<String, u64>>,
}

impl MidiInputManager {
  /// Create a new MIDI input manager
  pub fn new() -> Self {
    Self {
      connections: Mutex::new(HashMap::new()),
      parse_state: Arc::new(Mutex::new(MidiParseState::new())),
      desired_devices: Mutex::new(HashSet::new()),
      deferred_disconnects: Mutex::new(HashMap::new()),
    }
  }

  /// List available MIDI input ports
  pub fn list_ports() -> Vec<MidiPortInfo> {
    let midi_in = match MidiInput::new("modular-list") {
      Ok(m) => m,
      Err(_) => return Vec::new(),
    };

    midi_in
      .ports()
      .iter()
      .enumerate()
      .filter_map(|(index, port)| {
        midi_in
          .port_name(port)
          .ok()
          .map(|name| MidiPortInfo { name, index })
      })
      .collect()
  }

  /// Connect to a MIDI input port by name.
  /// Returns Ok(true) if newly connected, Ok(false) if already connected.
  pub fn connect(&self, port_name: &str) -> Result<bool, String> {
    // Add to desired devices for reconnection
    self.desired_devices.lock().insert(port_name.to_string());
    // Cancel any pending deferred close — the device is wanted again.
    self.deferred_disconnects.lock().remove(port_name);

    // Check if already connected
    if self.connections.lock().contains_key(port_name) {
      return Ok(false);
    }

    self.connect_internal(port_name)?;
    Ok(true)
  }

  /// Internal connection logic
  fn connect_internal(&self, port_name: &str) -> Result<(), String> {
    let midi_in =
      MidiInput::new("operator").map_err(|e| format!("Failed to create MIDI input: {}", e))?;

    // Find port by name
    let port = midi_in
      .ports()
      .into_iter()
      .find(|p| midi_in.port_name(p).ok().as_deref() == Some(port_name))
      .ok_or_else(|| format!("MIDI port '{}' not found", port_name))?;

    let parse_state = self.parse_state.clone();
    let device_name = port_name.to_string();

    let connection = midi_in
      .connect(
        &port,
        "modular-input",
        move |timestamp_us, data, _| {
          let mut state = parse_state.lock();
          if state.messages.len() < MIDI_BUFFER_SIZE {
            parse_midi_message(data, &device_name, timestamp_us, &mut state);
          }
        },
        (),
      )
      .map_err(|e| format!("Failed to connect to MIDI port '{}': {}", port_name, e))?;

    self
      .connections
      .lock()
      .insert(port_name.to_string(), connection);
    println!("[MIDI] Connected to: {}", port_name);

    Ok(())
  }

  /// Disconnect from all MIDI inputs
  pub fn disconnect_all(&self) {
    self.desired_devices.lock().clear();
    self.deferred_disconnects.lock().clear();
    let connections = std::mem::take(&mut *self.connections.lock());
    for (name, _conn) in connections {
      println!("[MIDI] Disconnected from: {}", name);
    }
  }

  /// Get list of currently connected port names
  pub fn connected_ports(&self) -> Vec<String> {
    self.connections.lock().keys().cloned().collect()
  }

  /// Get the name of a single connected port (for backward compatibility)
  /// Returns the first connected port, or None if no ports are connected
  pub fn connected_port(&self) -> Option<String> {
    self.connections.lock().keys().next().cloned()
  }

  /// Update desired devices and sync connections for a patch update.
  ///
  /// Opens devices the patch needs eagerly (opening early is harmless — messages
  /// from a device no module yet references are simply not dispatched). Closing a
  /// device the patch dropped is DEFERRED: it is recorded in `deferred_disconnects`
  /// against `update_id` and only closed once that update is applied (see
  /// `prune_disconnects`), so the still-playing patch keeps its MIDI input until
  /// the queued swap actually lands.
  pub fn sync_devices(&self, device_names: &HashSet<String>, update_id: u64) {
    let to_open = {
      let connected: HashSet<String> = self.connections.lock().keys().cloned().collect();
      let mut deferred = self.deferred_disconnects.lock();
      let mut desired = self.desired_devices.lock();
      plan_deferrals(&connected, device_names, update_id, &mut deferred, &mut desired)
    };

    // Locks released — `connect_internal` re-locks `connections`.
    for name in to_open {
      if !self.connections.lock().contains_key(&name) {
        if let Err(e) = self.connect_internal(&name) {
          eprintln!("[MIDI] Failed to connect to '{}': {}", name, e);
        }
      }
    }
  }

  /// Close devices whose deferred disconnect is now safe: the update that
  /// stopped referencing them has been applied (`uid <= applied_update_id`) and
  /// they are not wanted by the current patch. Runs on the main thread.
  /// Handles are removed under the `connections` lock but dropped after it is
  /// released — dropping a `MidiInputConnection` tears down the port and can
  /// block, so it must not happen while holding the lock.
  pub fn prune_disconnects(&self, applied_update_id: u64) {
    let to_close = {
      let mut deferred = self.deferred_disconnects.lock();
      let desired = self.desired_devices.lock();
      plan_prunes(&mut deferred, &desired, applied_update_id)
    };
    if to_close.is_empty() {
      return;
    }

    let mut doomed: Vec<(String, MidiInputConnection<()>)> = Vec::new();
    {
      let mut connections = self.connections.lock();
      for name in &to_close {
        if let Some(conn) = connections.remove(name) {
          doomed.push((name.clone(), conn));
        }
      }
    }
    // Connections drop here, with no manager lock held.
    for (name, _conn) in doomed {
      println!("[MIDI] Disconnected from: {} (deferred close applied)", name);
    }
  }

  /// Attempt to reconnect to any desired devices that aren't currently connected.
  /// Call this periodically to handle hot-plugged devices.
  pub fn try_reconnect(&self) {
    let desired: Vec<String> = self.desired_devices.lock().iter().cloned().collect();

    for name in desired {
      if !self.connections.lock().contains_key(&name) {
        // Try to reconnect silently (device may not be plugged in)
        if let Ok(()) = self.connect_internal(&name) {
          println!("[MIDI] Reconnected to: {}", name);
        }
      }
    }
  }

  /// Take all pending messages, sorted by timestamp (clears the buffer).
  /// Messages from multiple devices are interleaved in the correct temporal order.
  pub fn take_messages(&self) -> Vec<Message> {
    let mut state = self.parse_state.lock();
    let mut timestamped = std::mem::take(&mut state.messages);

    // Sort by timestamp to ensure messages are processed in the correct order
    // even when coming from multiple MIDI devices
    timestamped.sort_by_key(|m| m.timestamp_us);

    // Extract just the messages, now in correct order
    timestamped.into_iter().map(|tm| tm.message).collect()
  }
}

impl Default for MidiInputManager {
  fn default() -> Self {
    Self::new()
  }
}

/// Parse raw MIDI bytes and add messages to state.
/// Handles 14-bit CC by tracking MSB (CC 0-31) and combining with LSB (CC 32-63).
fn parse_midi_message(data: &[u8], device: &str, timestamp_us: u64, state: &mut MidiParseState) {
  if data.is_empty() {
    return;
  }

  let status_byte = data[0];

  // Skip system messages (0xF0-0xFF)
  if status_byte >= 0xF0 {
    return;
  }

  let channel = status_byte & 0x0F;
  let status = status_byte & 0xF0;
  let data1 = data.get(1).copied().unwrap_or(0);
  let data2 = data.get(2).copied().unwrap_or(0);
  let device_opt = Some(device.to_string());

  let message = match status {
    0x90 if data2 > 0 => {
      // Note On
      Some(Message::MidiNoteOn(MidiNoteOn {
        device: device_opt,
        channel,
        note: data1,
        velocity: data2,
      }))
    }
    0x80 | 0x90 => {
      // Note Off (or Note On with velocity 0)
      Some(Message::MidiNoteOff(MidiNoteOff {
        device: device_opt,
        channel,
        note: data1,
        velocity: data2,
      }))
    }
    0xB0 => {
      // Control Change - handle 14-bit CC
      let cc = data1;
      let value = data2;
      let cc_state = state.get_cc_state(device);

      if cc < 32 {
        // MSB for 14-bit CC (CC 0-31)
        // Store MSB and emit regular 7-bit CC message
        // The 14-bit message will be emitted when LSB arrives
        cc_state.set_msb(channel, cc, value);
        Some(Message::MidiCC(MidiControlChange {
          device: device_opt,
          channel,
          cc,
          value,
        }))
      } else if cc < 64 {
        // LSB for 14-bit CC (CC 32-63)
        // Check if we have a stored MSB
        let cc_msb = cc - 32;
        if let Some(msb) = cc_state.take_msb(channel, cc_msb) {
          // Combine MSB and LSB into 14-bit value
          let value_14bit = ((msb as u16) << 7) | (value as u16);
          // Emit both the regular LSB CC message and the 14-bit message
          state.messages.push(TimestampedMessage {
            timestamp_us,
            message: Message::MidiCC(MidiControlChange {
              device: device_opt.clone(),
              channel,
              cc,
              value,
            }),
          });
          Some(Message::MidiCC14Bit(MidiControlChange14Bit {
            device: device_opt,
            channel,
            cc: cc_msb,
            value: value_14bit,
          }))
        } else {
          // No MSB stored, just emit regular CC
          Some(Message::MidiCC(MidiControlChange {
            device: device_opt,
            channel,
            cc,
            value,
          }))
        }
      } else {
        // Regular CC (64-127)
        Some(Message::MidiCC(MidiControlChange {
          device: device_opt,
          channel,
          cc,
          value,
        }))
      }
    }
    0xE0 => {
      // Pitch Bend
      let value = (((data2 as u16) << 7) | (data1 as u16)) as i16 - 8192;
      Some(Message::MidiPitchBend(MidiPitchBend {
        device: device_opt,
        channel,
        value,
      }))
    }
    0xD0 => {
      // Channel Pressure (Aftertouch)
      Some(Message::MidiChannelPressure(MidiChannelPressure {
        device: device_opt,
        channel,
        pressure: data1,
      }))
    }
    0xA0 => {
      // Polyphonic Key Pressure
      Some(Message::MidiPolyPressure(MidiPolyPressure {
        device: device_opt,
        channel,
        note: data1,
        pressure: data2,
      }))
    }
    _ => None,
  };

  if let Some(msg) = message {
    state.messages.push(TimestampedMessage {
      timestamp_us,
      message: msg,
    });
  }
}

/// Pure bookkeeping for `sync_devices`: record deferred closes for dropped
/// devices, cancel deferrals for re-referenced ones, set `desired`, and return
/// the devices to open (wanted − currently connected). Kept free of MIDI I/O so
/// it is unit-testable without real ports.
fn plan_deferrals(
  connected: &HashSet<String>,
  device_names: &HashSet<String>,
  update_id: u64,
  deferred: &mut HashMap<String, u64>,
  desired: &mut HashSet<String>,
) -> Vec<String> {
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
fn plan_prunes(
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

#[cfg(test)]
mod tests {
  use super::{plan_deferrals, plan_prunes};
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
}
