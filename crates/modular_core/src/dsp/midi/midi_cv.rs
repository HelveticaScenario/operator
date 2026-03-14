//! MIDI to CV converter module with polyphonic voice allocation.
//!
//! Converts MIDI note messages to pitch CV and gate signals with configurable
//! voice allocation modes following VCV Rack conventions.

use napi::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dsp::utils::{
    min_gate_samples, TempGate, TempGateState, GATE_HIGH_VOLTAGE, GATE_LOW_VOLTAGE,
};
use crate::patch::Patch;
use crate::poly::{PolyOutput, PORT_MAX_CHANNELS};
use crate::types::{
    Connect, MidiChannelPressure, MidiControlChange, MidiNoteOff, MidiNoteOn, MidiPitchBend,
    MidiPolyPressure,
};

/// Voice allocation mode for polyphonic operation
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PolyMode {
    /// Round-robin through available voices
    #[default]
    Rotate,
    /// Reuse voice playing same note before rotating
    Reuse,
    /// Always start from the first voice
    Reset,
    /// MPE mode: MIDI channel maps directly to output channel
    Mpe,
}

impl Connect for PolyMode {
    fn connect(&mut self, _patch: &Patch) {}
}

/// Note priority for monophonic operation
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MonoMode {
    /// Last note pressed wins
    #[default]
    Last,
    /// First note pressed wins (ignores new notes)
    First,
    /// Lowest pitch note wins
    Lowest,
    /// Highest pitch note wins
    Highest,
}

impl Connect for MonoMode {
    fn connect(&mut self, _patch: &Patch) {}
}

/// State for a single voice
#[derive(Debug, Clone, Copy, Default)]
struct VoiceState {
    /// MIDI note number (0-127)
    note: u8,
    /// Velocity (0-127)
    velocity: u8,
    /// Gate state
    gate: bool,
    /// Aftertouch (0-127)
    aftertouch: u8,
    /// Pitch wheel value (-8192 to 8191)
    pitch_wheel: i16,
    /// Mod wheel value (0-127)
    mod_wheel: u8,
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct MidiCvParams {
    /// MIDI device name to receive from (leave unset to receive from all devices)
    device: Option<String>,

    /// Number of polyphonic voices (1-16)
    #[serde(default = "default_channels")]
    channels: usize,

    /// MIDI channel to listen on (1–16, leave unset for omni/all channels)
    channel: Option<u8>,

    /// Polyphonic voice allocation mode
    #[serde(default)]
    poly_mode: PolyMode,

    /// Monophonic note priority (used when only one voice is active)
    #[serde(default)]
    mono_mode: MonoMode,

    /// Pitch bend range in semitones (0 = disabled, default 2)
    #[serde(default = "default_pitch_bend_range")]
    pitch_bend_range: u8,
}

fn default_channels() -> usize {
    1
}

fn default_pitch_bend_range() -> u8 {
    2
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MidiCvOutputs {
    #[output(
        "pitch",
        "pitch CV in 1V/octave (0V = C4, includes pitch bend)",
        default
    )]
    pitch: PolyOutput,
    #[output("gate", "gate output (0V or 5V)", range = (0.0, 5.0))]
    gate: PolyOutput,
    #[output("velocity", "velocity (0-5V)", range = (0.0, 5.0))]
    velocity: PolyOutput,
    #[output("aftertouch", "channel pressure / aftertouch (0-5V)", range = (0.0, 5.0))]
    aftertouch: PolyOutput,
    #[output("retrigger", "retrigger pulse (5V for 1ms on new note)", range = (0.0, 5.0))]
    retrigger: PolyOutput,
    #[output("pitchWheel", "pitch wheel position (-5V to +5V)", range = (-5.0, 5.0))]
    pitch_wheel: PolyOutput,
    #[output("modWheel", "mod wheel (0-5V)", range = (0.0, 5.0))]
    mod_wheel: PolyOutput,
}

/// Converts MIDI note input into control voltages for driving synthesizer modules.
/// Supports polyphonic voice allocation with up to 16 voices.
///
/// ## Outputs
///
/// | Output | Signal | Range |
/// |---|---|---|
/// | `pitch` | V/Oct pitch (0V = C4), includes pitch bend | — |
/// | `gate` | High while a note is held | 0–5V |
/// | `velocity` | Note-on velocity | 0–5V |
/// | `aftertouch` | Channel or polyphonic aftertouch | 0–5V |
/// | `retrigger` | 5V pulse for 1ms on each new note | 0–5V |
/// | `pitchWheel` | Pitch bend wheel position | -5V–+5V |
/// | `modWheel` | Mod wheel (CC 1) | 0–5V |
///
/// ## Polyphonic modes (`polyMode`)
///
/// - `"rotate"` — cycles through voices round-robin
/// - `"reuse"` — reuses the voice that last played the same note
/// - `"reset"` — always starts allocation from voice 1
/// - `"mpe"` — one voice per MIDI channel (for MPE controllers)
///
/// ## Monophonic modes (`monoMode`, when `channels: 1`)
///
/// - `"last"` — most recently pressed note wins
/// - `"first"` — first pressed note wins
/// - `"lowest"` — lowest held note wins
/// - `"highest"` — highest held note wins
///
/// ## Example
///
/// ```js
/// // 4-voice polyphonic MIDI synth
/// const midi = $midiCV({ channels: 4 });
/// $sine(midi.pitch).amplitude($adsr(midi.gate,{attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.5})).out();
///
/// // Mono lead with pitch bend
/// const lead = $midiCV({ channels: 1, pitchBendRange: 12 });
/// $saw(lead.pitch).amplitude($adsr(lead.gate,{attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.5})).out();
/// ```
#[module(name = "$midiCV", channels_param = "channels", args())]
pub struct MidiCv {
    outputs: MidiCvOutputs,
    params: MidiCvParams,
    state: MidiCvState,
}

/// State for the MidiCv module.
pub struct MidiCvState {
    sample_rate: f32,

    /// Per-voice state
    voices: [VoiceState; PORT_MAX_CHANNELS],

    /// Held notes (order preserved for priority modes)
    held_notes: Vec<(u8, u8, u8)>, // (note, velocity, midi_channel)

    /// Current rotation index for voice allocation
    rotate_index: usize,

    /// Sustain pedal state per MIDI channel
    sustain: [bool; 16],

    /// Notes held by sustain (note, velocity, midi_channel)
    sustained_notes: Vec<(u8, u8, u8)>,

    /// Global pitch wheel (for non-MPE mode)
    global_pitch_wheel: i16,

    /// Global mod wheel (for non-MPE mode)
    global_mod_wheel: u8,

    /// Global aftertouch (for non-MPE mode)
    global_aftertouch: u8,

    /// Retrigger pulse gates (multi-sample duration via TempGate)
    retrigger_gates: [TempGate; PORT_MAX_CHANNELS],

    last_channel_count: usize,
}

impl Default for MidiCvState {
    fn default() -> Self {
        Self {
            sample_rate: 48000.0,
            voices: [VoiceState::default(); PORT_MAX_CHANNELS],
            held_notes: Vec::with_capacity(128),
            rotate_index: 0,
            sustain: [false; 16],
            sustained_notes: Vec::with_capacity(128),
            global_pitch_wheel: 0,
            global_mod_wheel: 0,
            global_aftertouch: 0,
            retrigger_gates: [TempGate::default(); PORT_MAX_CHANNELS],
            last_channel_count: 0,
        }
    }
}

impl MidiCv {
    /// Convert MIDI note number to 1V/octave CV (0V = C4 = MIDI 60)
    fn note_to_cv(note: u8) -> f32 {
        (note as f32 - 60.0) / 12.0
    }

    /// Convert velocity (0-127) to voltage (0-5V)
    fn velocity_to_cv(velocity: u8) -> f32 {
        velocity as f32 / 127.0 * 5.0
    }

    /// Convert pitch bend to voltage offset based on range
    fn pitch_bend_to_cv(&self, pitch_bend: i16) -> f32 {
        if self.params.pitch_bend_range == 0 {
            return 0.0;
        }
        // pitch_bend is -8192 to 8191
        // Convert to semitones based on range, then to volts (1V/octave)
        let semitones = (pitch_bend as f32 / 8192.0) * self.params.pitch_bend_range as f32;
        semitones / 12.0
    }

    /// Check if we should process events from a MIDI device
    fn should_process_device(&self, device: Option<&String>) -> bool {
        match (&self.params.device, device) {
            (None, _) => true,                          // No filter = accept all devices
            (Some(wanted), Some(got)) => wanted == got, // Exact match
            (Some(_), None) => false,                   // Filter set but no device info
        }
    }

    /// Check if we should process events from a MIDI channel
    fn should_process_channel(&self, midi_channel: u8) -> bool {
        match self.params.channel {
            None => true,                                       // Omni mode
            Some(ch) => midi_channel == (ch.saturating_sub(1)), // 1-indexed param to 0-indexed MIDI
        }
    }

    /// Find a free voice or steal one based on poly mode
    fn allocate_voice(&mut self, note: u8, midi_channel: u8) -> usize {
        let num_voices = self.params.channels.clamp(1, PORT_MAX_CHANNELS) as usize;

        match self.params.poly_mode {
            PolyMode::Mpe => {
                // In MPE mode, MIDI channel directly maps to voice
                // Channel 0 (master) typically maps to all voices, channels 1-15 map to voices 0-14
                if midi_channel == 0 {
                    0 // Master channel controls first voice
                } else {
                    ((midi_channel - 1) as usize).min(num_voices - 1)
                }
            }
            PolyMode::Reuse => {
                // First, try to find a voice already playing this note
                for i in 0..num_voices {
                    if self.state.voices[i].gate && self.state.voices[i].note == note {
                        return i;
                    }
                }
                // Fall through to rotate behavior
                self.allocate_voice_rotate(num_voices)
            }
            PolyMode::Reset => {
                // Always scan from 0, use first free voice
                for i in 0..num_voices {
                    if !self.state.voices[i].gate {
                        return i;
                    }
                }
                // All busy: use last voice
                num_voices - 1
            }
            PolyMode::Rotate => self.allocate_voice_rotate(num_voices),
        }
    }

    fn allocate_voice_rotate(&mut self, num_voices: usize) -> usize {
        // Find next free voice starting from rotate_index
        for i in 0..num_voices {
            let idx = (self.state.rotate_index + i) % num_voices;
            if !self.state.voices[idx].gate {
                self.state.rotate_index = (idx + 1) % num_voices;
                return idx;
            }
        }
        // All voices busy: steal from rotate_index
        let idx = self.state.rotate_index;
        self.state.rotate_index = (idx + 1) % num_voices;
        idx
    }

    /// Find which voice is playing a note
    fn find_voice_for_note(&self, note: u8) -> Option<usize> {
        let num_voices = self.params.channels.clamp(1, PORT_MAX_CHANNELS) as usize;
        for i in 0..num_voices {
            if self.state.voices[i].gate && self.state.voices[i].note == note {
                return Some(i);
            }
        }
        None
    }

    /// Get the note to play in monophonic mode based on priority
    fn get_mono_note(&self) -> Option<(u8, u8)> {
        if self.state.held_notes.is_empty() {
            return None;
        }

        match self.params.mono_mode {
            MonoMode::Last => self.state.held_notes.last().map(|&(n, v, _)| (n, v)),
            MonoMode::First => self.state.held_notes.first().map(|&(n, v, _)| (n, v)),
            MonoMode::Lowest => self
                .state
                .held_notes
                .iter()
                .min_by_key(|&&(n, _, _)| n)
                .map(|&(n, v, _)| (n, v)),
            MonoMode::Highest => self
                .state
                .held_notes
                .iter()
                .max_by_key(|&&(n, _, _)| n)
                .map(|&(n, v, _)| (n, v)),
        }
    }

    /// Handle MIDI note on message
    fn on_midi_note_on(&mut self, msg: &MidiNoteOn) -> Result<()> {
        if !self.should_process_device(msg.device.as_ref())
            || !self.should_process_channel(msg.channel)
        {
            return Ok(());
        }

        let note = msg.note;
        let velocity = msg.velocity;
        let midi_channel = msg.channel;

        // Remove from held_notes if already present (shouldn't happen but be safe)
        self.state.held_notes.retain(|&(n, _, _)| n != note);
        self.state.held_notes.push((note, velocity, midi_channel));

        let num_voices = self.params.channels.clamp(1, PORT_MAX_CHANNELS) as usize;

        if num_voices == 1 {
            // Monophonic mode
            if let Some((mono_note, mono_vel)) = self.get_mono_note() {
                let voice = &mut self.state.voices[0];
                let should_retrigger = !voice.gate || voice.note != mono_note;
                voice.note = mono_note;
                voice.velocity = mono_vel;
                voice.gate = true;
                if should_retrigger {
                    self.state.retrigger_gates[0].set_state(
                        TempGateState::High,
                        TempGateState::Low,
                        min_gate_samples(self.state.sample_rate),
                    );
                }
            }
        } else {
            // Polyphonic mode
            let voice_idx = self.allocate_voice(note, midi_channel);
            let voice = &mut self.state.voices[voice_idx];
            voice.note = note;
            voice.velocity = velocity;
            voice.gate = true;
            self.state.retrigger_gates[voice_idx].set_state(
                TempGateState::High,
                TempGateState::Low,
                min_gate_samples(self.state.sample_rate),
            );
        }

        Ok(())
    }

    /// Handle MIDI note off message
    fn on_midi_note_off(&mut self, msg: &MidiNoteOff) -> Result<()> {
        if !self.should_process_device(msg.device.as_ref())
            || !self.should_process_channel(msg.channel)
        {
            return Ok(());
        }

        let note = msg.note;
        let midi_channel = msg.channel;

        // Check if sustain pedal is held for this MIDI channel
        let ch_idx = midi_channel as usize;
        if ch_idx < 16 && self.state.sustain[ch_idx] {
            // Move to sustained notes instead of releasing
            if let Some(idx) = self
                .state
                .held_notes
                .iter()
                .position(|&(n, _, _)| n == note)
            {
                let (n, v, c) = self.state.held_notes.remove(idx);
                self.state.sustained_notes.push((n, v, c));
            }
            return Ok(());
        }

        // Remove from held notes
        self.state.held_notes.retain(|&(n, _, _)| n != note);
        self.state.sustained_notes.retain(|&(n, _, _)| n != note);

        let num_voices = self.params.channels.clamp(1, PORT_MAX_CHANNELS) as usize;

        if num_voices == 1 {
            // Monophonic: check if a different note should take over
            if let Some((mono_note, mono_vel)) = self.get_mono_note() {
                let voice = &mut self.state.voices[0];
                if voice.note != mono_note {
                    voice.note = mono_note;
                    voice.velocity = mono_vel;
                    self.state.retrigger_gates[0].set_state(
                        TempGateState::High,
                        TempGateState::Low,
                        min_gate_samples(self.state.sample_rate),
                    );
                }
            } else {
                self.state.voices[0].gate = false;
            }
        } else {
            // Polyphonic: release the specific voice
            if let Some(voice_idx) = self.find_voice_for_note(note) {
                self.state.voices[voice_idx].gate = false;
            }
        }

        Ok(())
    }

    /// Handle MIDI CC message (for sustain pedal and mod wheel)
    fn on_midi_cc(&mut self, msg: &MidiControlChange) -> Result<()> {
        if !self.should_process_device(msg.device.as_ref())
            || !self.should_process_channel(msg.channel)
        {
            return Ok(());
        }

        match msg.cc {
            1 => {
                // Mod wheel
                if self.params.poly_mode == PolyMode::Mpe && msg.channel > 0 {
                    let voice_idx = ((msg.channel - 1) as usize)
                        .min(self.params.channels.saturating_sub(1) as usize);
                    self.state.voices[voice_idx].mod_wheel = msg.value;
                } else {
                    self.state.global_mod_wheel = msg.value;
                }
            }
            64 => {
                // Sustain pedal
                let held = msg.value >= 64;
                let ch_idx = msg.channel as usize;
                if ch_idx < 16 {
                    let was_held = self.state.sustain[ch_idx];
                    self.state.sustain[ch_idx] = held;

                    // When sustain is released, release all sustained notes for this channel
                    if was_held && !held {
                        let notes_to_release: Vec<u8> = self
                            .state
                            .sustained_notes
                            .iter()
                            .filter(|&&(_, _, c)| c == msg.channel)
                            .map(|&(n, _, _)| n)
                            .collect();

                        self.state
                            .sustained_notes
                            .retain(|&(_, _, c)| c != msg.channel);

                        // Release voices for these notes
                        for note in notes_to_release {
                            if let Some(voice_idx) = self.find_voice_for_note(note) {
                                self.state.voices[voice_idx].gate = false;
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle MIDI pitch bend message
    fn on_midi_pitch_bend(&mut self, msg: &MidiPitchBend) -> Result<()> {
        if !self.should_process_device(msg.device.as_ref())
            || !self.should_process_channel(msg.channel)
        {
            return Ok(());
        }

        if self.params.poly_mode == PolyMode::Mpe && msg.channel > 0 {
            // MPE: per-voice pitch bend
            let voice_idx =
                ((msg.channel - 1) as usize).min(self.params.channels.saturating_sub(1) as usize);
            self.state.voices[voice_idx].pitch_wheel = msg.value;
        } else {
            // Standard: global pitch bend
            self.state.global_pitch_wheel = msg.value;
        }

        Ok(())
    }

    /// Handle MIDI channel pressure message
    fn on_midi_channel_pressure(&mut self, msg: &MidiChannelPressure) -> Result<()> {
        if !self.should_process_device(msg.device.as_ref())
            || !self.should_process_channel(msg.channel)
        {
            return Ok(());
        }

        if self.params.poly_mode == PolyMode::Mpe && msg.channel > 0 {
            let voice_idx =
                ((msg.channel - 1) as usize).min(self.params.channels.saturating_sub(1) as usize);
            self.state.voices[voice_idx].aftertouch = msg.pressure;
        } else {
            self.state.global_aftertouch = msg.pressure;
        }

        Ok(())
    }

    /// Handle MIDI polyphonic pressure message
    fn on_midi_poly_pressure(&mut self, msg: &MidiPolyPressure) -> Result<()> {
        if !self.should_process_device(msg.device.as_ref())
            || !self.should_process_channel(msg.channel)
        {
            return Ok(());
        }

        // Find voice playing this note and update its aftertouch
        if let Some(voice_idx) = self.find_voice_for_note(msg.note) {
            self.state.voices[voice_idx].aftertouch = msg.pressure;
        }

        Ok(())
    }

    /// Handle MIDI panic
    fn on_midi_panic(&mut self) -> Result<()> {
        self.state.held_notes.clear();
        self.state.sustained_notes.clear();
        self.state.rotate_index = 0;
        self.state.global_pitch_wheel = 0;
        self.state.global_mod_wheel = 0;
        self.state.global_aftertouch = 0;

        for i in 0..PORT_MAX_CHANNELS {
            self.state.voices[i] = VoiceState::default();
            self.state.retrigger_gates[i].set_state(TempGateState::Low, TempGateState::Low, 0);
        }

        for i in 0..16 {
            self.state.sustain[i] = false;
        }

        Ok(())
    }

    fn update(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;

        let num_voices = self.channel_count();
        if self.state.last_channel_count != num_voices {
            self.state.last_channel_count = num_voices;
            println!("MIDI CV: updating to {} voices", num_voices);
        }

        // Update outputs for each voice
        for i in 0..num_voices as usize {
            let voice = &self.state.voices[i];

            // Pitch CV
            let pitch_cv = Self::note_to_cv(voice.note);
            let pitch_bend_cv = if self.params.poly_mode == PolyMode::Mpe {
                self.pitch_bend_to_cv(voice.pitch_wheel)
            } else {
                self.pitch_bend_to_cv(self.state.global_pitch_wheel)
            };
            self.outputs.pitch.set(i, pitch_cv + pitch_bend_cv);

            // Gate
            self.outputs.gate.set(
                i,
                if voice.gate {
                    GATE_HIGH_VOLTAGE
                } else {
                    GATE_LOW_VOLTAGE
                },
            );

            // Velocity
            self.outputs
                .velocity
                .set(i, Self::velocity_to_cv(voice.velocity));

            // Aftertouch
            let aftertouch = if self.params.poly_mode == PolyMode::Mpe {
                voice.aftertouch
            } else {
                self.state.global_aftertouch.max(voice.aftertouch)
            };
            self.outputs
                .aftertouch
                .set(i, aftertouch as f32 / 127.0 * 5.0);

            // Retrigger pulse
            self.outputs
                .retrigger
                .set(i, self.state.retrigger_gates[i].process());

            // Pitch wheel (raw, unscaled, -5V to +5V)
            let pw = if self.params.poly_mode == PolyMode::Mpe {
                voice.pitch_wheel
            } else {
                self.state.global_pitch_wheel
            };
            self.outputs.pitch_wheel.set(i, pw as f32 / 8192.0 * 5.0);

            // Mod wheel
            let mw = if self.params.poly_mode == PolyMode::Mpe {
                voice.mod_wheel
            } else {
                self.state.global_mod_wheel
            };
            self.outputs.mod_wheel.set(i, mw as f32 / 127.0 * 5.0);
        }
    }
}

message_handlers!(impl MidiCv {
    MidiNoteOn(m) => MidiCv::on_midi_note_on,
    MidiNoteOff(m) => MidiCv::on_midi_note_off,
    MidiCC(m) => MidiCv::on_midi_cc,
    MidiPitchBend(m) => MidiCv::on_midi_pitch_bend,
    MidiChannelPressure(m) => MidiCv::on_midi_channel_pressure,
    MidiPolyPressure(m) => MidiCv::on_midi_poly_pressure,
    MidiPanic => MidiCv::on_midi_panic,
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_to_cv() {
        // C4 = MIDI 60 = 0V
        assert!((MidiCv::note_to_cv(60) - 0.0).abs() < 0.001);
        // C5 = MIDI 72 = 1V
        assert!((MidiCv::note_to_cv(72) - 1.0).abs() < 0.001);
        // C3 = MIDI 48 = -1V
        assert!((MidiCv::note_to_cv(48) - (-1.0)).abs() < 0.001);
    }

    #[test]
    fn test_velocity_to_cv() {
        assert!((MidiCv::velocity_to_cv(0) - 0.0).abs() < 0.001);
        assert!((MidiCv::velocity_to_cv(127) - 5.0).abs() < 0.001);
        assert!((MidiCv::velocity_to_cv(64) - 2.52).abs() < 0.1);
    }
}
