//! MIDI input handling for the modular synthesizer.
//!
//! This module provides MIDI device enumeration, connection management,
//! and converts raw MIDI bytes to Message types for dispatch to DSP modules.
//! Supports multiple simultaneous MIDI device connections.
//! Messages are timestamped and sorted to ensure correct ordering.
//! Supports 14-bit CC messages (CC 0-31 MSB + CC 32-63 LSB).

mod devices;
mod parse;

use devices::{plan_deferrals, plan_prunes};
use midir::{MidiInput, MidiInputConnection};
use modular_core::types::{DeviceName, Message, MidiNoteOff};
use parking_lot::Mutex;
use parse::{MidiParseState, parse_midi_message};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Maximum MIDI messages buffered. Also the capacity of the audio thread's
/// drain scratch — the two vectors swap wholesale, and neither side ever
/// pushes past this bound, so neither ever grows.
pub const MIDI_BUFFER_SIZE: usize = 1024;

/// A MIDI message with its timestamp (microseconds from midir)
#[derive(Debug, Clone)]
pub struct TimestampedMessage {
    /// Timestamp in microseconds (from midir callback)
    pub timestamp_us: u64,
    /// Arrival order within this manager. Tie-breaks equal timestamps so the
    /// audio thread's in-place unstable sort is still a total, deterministic
    /// order (e.g. a note-off and note-on landing on the same microsecond).
    pub seq: u64,
    /// The parsed MIDI message
    pub message: Message,
}

/// Information about a MIDI input port
#[derive(Debug, Clone)]
pub struct MidiPortInfo {
    pub name: String,
    pub index: usize,
}

/// Manages multiple MIDI input connections
pub struct MidiInputManager {
    /// Active connections keyed by device name
    connections: Mutex<HashMap<String, MidiInputConnection<()>>>,
    /// Shared parse state (messages, 14-bit CC and held-note tracking)
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
        let midi_in = MidiInput::new("operator")
            .map_err(|e| format!("Failed to create MIDI input: {}", e))?;

        // Find port by name
        let port = midi_in
            .ports()
            .into_iter()
            .find(|p| midi_in.port_name(p).ok().as_deref() == Some(port_name))
            .ok_or_else(|| format!("MIDI port '{}' not found", port_name))?;

        let parse_state = self.parse_state.clone();
        // Interned once per connection; per-message clones are refcount-only,
        // so dropping dispatched messages on the audio thread never frees heap.
        let device_name = DeviceName::intern(port_name);

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

        self.connections
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
        let mut closed: Vec<String> = Vec::new();
        for (name, _conn) in connections {
            println!("[MIDI] Disconnected from: {}", name);
            closed.push(name);
        }
        // All connections are torn down, so no more messages can arrive from
        // them and the synthesized offs are guaranteed to sort last.
        for name in &closed {
            self.push_note_offs_for_device(name);
        }
    }

    /// Synthesize note-offs for every note held on `device`, delivered through
    /// the pending-message queue — the same path real device messages take —
    /// so a closed device cannot leave notes latched in modules. Timestamped
    /// at the newest pending message so the offs sort after anything the
    /// device already queued.
    fn push_note_offs_for_device(&self, device: &str) {
        let mut state = self.parse_state.lock();
        let Some(held) = state.held_notes.remove(device) else {
            return;
        };
        if held.is_empty() {
            return;
        }
        let timestamp_us = state
            .messages
            .iter()
            .map(|m| m.timestamp_us)
            .max()
            .unwrap_or(0);
        let device_name = DeviceName::intern(device);
        for (channel, note) in held {
            state.push(
                timestamp_us,
                Message::MidiNoteOff(MidiNoteOff {
                    device: Some(device_name.clone()),
                    channel,
                    note,
                    velocity: 0,
                }),
            );
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
    ///
    /// `wants_all_devices` marks a patch containing a deviceless MIDI module
    /// (which receives from every device): every open or previously desired
    /// device stays wanted, and no closes are scheduled.
    pub fn sync_devices(
        &self,
        device_names: &HashSet<String>,
        wants_all_devices: bool,
        update_id: u64,
    ) {
        let to_open = {
            let connected: HashSet<String> = self.connections.lock().keys().cloned().collect();
            let mut deferred = self.deferred_disconnects.lock();
            let mut desired = self.desired_devices.lock();
            plan_deferrals(
                &connected,
                device_names,
                wants_all_devices,
                update_id,
                &mut deferred,
                &mut desired,
            )
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
            println!(
                "[MIDI] Disconnected from: {} (deferred close applied)",
                name
            );
        }
        // Clear held notes for every pruned device, including ones whose
        // connection had already vanished (e.g. unplugged before the prune).
        for name in &to_close {
            self.push_note_offs_for_device(name);
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

    /// Move all pending messages into `out` (which must be empty) by swapping
    /// the two backing buffers, so the caller — the audio thread — never
    /// allocates. Messages from multiple devices arrive interleaved; the caller
    /// sorts by `(timestamp_us, seq)` for correct temporal order. Uses
    /// `try_lock` so a MIDI-thread writer never blocks the audio thread; on
    /// contention the messages simply wait for the next poll.
    pub fn take_messages_into(&self, out: &mut Vec<TimestampedMessage>) {
        debug_assert!(out.is_empty());
        if let Some(mut state) = self.parse_state.try_lock() {
            std::mem::swap(&mut state.messages, out);
        }
    }
}

impl Default for MidiInputManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
