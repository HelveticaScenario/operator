//! Raw MIDI byte parsing and the shared parse state it feeds: the pending
//! message queue, 14-bit CC MSB tracking, and held-note bookkeeping.

use super::{MIDI_BUFFER_SIZE, TimestampedMessage};
use modular_core::types::{
    DeviceName, Message, MidiChannelPressure, MidiControlChange, MidiControlChange14Bit,
    MidiNoteOff, MidiNoteOn, MidiPitchBend, MidiPolyPressure,
};
use std::collections::{HashMap, HashSet};

/// State for tracking 14-bit CC MSB values per device
/// Key: (device_name, channel, cc_msb), Value: msb_value
#[derive(Debug, Default)]
pub(super) struct MidiCcState {
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

/// Shared state for MIDI parsing across callbacks
pub(super) struct MidiParseState {
    /// Pending messages with timestamps
    pub(super) messages: Vec<TimestampedMessage>,
    /// Next value for [`TimestampedMessage::seq`]
    next_seq: u64,
    /// 14-bit CC state per device
    cc_state: HashMap<String, MidiCcState>,
    /// Notes currently held per device, as (channel, note). Consulted when a
    /// device closes to synthesize the note-offs it can no longer send.
    pub(super) held_notes: HashMap<String, HashSet<(u8, u8)>>,
}

impl MidiParseState {
    pub(super) fn new() -> Self {
        Self {
            messages: Vec::with_capacity(MIDI_BUFFER_SIZE),
            next_seq: 0,
            cc_state: HashMap::new(),
            held_notes: HashMap::new(),
        }
    }

    /// Queue a parsed message, dropping it if the buffer is at capacity.
    /// The cap is enforced here — not only per packet in the connection
    /// callbacks — because one packet can produce two messages (the 14-bit CC
    /// path), and device input must never grow the buffer past
    /// `MIDI_BUFFER_SIZE`.
    pub(super) fn push(&mut self, timestamp_us: u64, message: Message) {
        if self.messages.len() >= MIDI_BUFFER_SIZE {
            return;
        }
        self.push_unchecked(timestamp_us, message);
    }

    /// Queue a message bypassing the capacity cap. Reserved for the note-offs
    /// synthesized when a device closes: dropping one would leave a note
    /// latched forever with no device left to release it. The overage is
    /// bounded by the device's held-note set, and any growth allocates on the
    /// thread doing the close — the audio thread only ever swaps the buffer
    /// out, so it never reallocates or frees it.
    pub(super) fn push_unchecked(&mut self, timestamp_us: u64, message: Message) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.messages.push(TimestampedMessage {
            timestamp_us,
            seq,
            message,
        });
    }

    fn get_cc_state(&mut self, device: &str) -> &mut MidiCcState {
        self.cc_state
            .entry(device.to_string())
            .or_insert_with(MidiCcState::new)
    }

    fn track_note_on(&mut self, device: &str, channel: u8, note: u8) {
        self.held_notes
            .entry(device.to_string())
            .or_default()
            .insert((channel, note));
    }

    fn track_note_off(&mut self, device: &str, channel: u8, note: u8) {
        if let Some(held) = self.held_notes.get_mut(device) {
            held.remove(&(channel, note));
        }
    }
}

/// Parse raw MIDI bytes and add messages to state.
/// Handles 14-bit CC by tracking MSB (CC 0-31) and combining with LSB (CC 32-63).
pub(super) fn parse_midi_message(
    data: &[u8],
    device: &DeviceName,
    timestamp_us: u64,
    state: &mut MidiParseState,
) {
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
    let device_opt = Some(device.clone());

    let message = match status {
        0x90 if data2 > 0 => {
            // Note On
            state.track_note_on(device.as_str(), channel, data1);
            Some(Message::MidiNoteOn(MidiNoteOn {
                device: device_opt,
                channel,
                note: data1,
                velocity: data2,
            }))
        }
        0x80 | 0x90 => {
            // Note Off (or Note On with velocity 0)
            state.track_note_off(device.as_str(), channel, data1);
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
            let cc_state = state.get_cc_state(device.as_str());

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
                    state.push(
                        timestamp_us,
                        Message::MidiCC(MidiControlChange {
                            device: device_opt.clone(),
                            channel,
                            cc,
                            value,
                        }),
                    );
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
        state.push(timestamp_us, msg);
    }
}
