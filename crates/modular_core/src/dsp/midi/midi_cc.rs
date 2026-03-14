//! MIDI CC (Control Change) to CV converter module.
//!
//! Converts MIDI CC messages to control voltage signals.
//! Supports both 7-bit (standard) and 14-bit (high-resolution) CC.

use napi::Result;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::types::{MidiControlChange, MidiControlChange14Bit};

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct MidiCcParams {
    /// MIDI device name to receive from (leave unset to receive from all devices)
    device: Option<String>,

    /// CC number to monitor (0-127 for 7-bit, 0-31 for 14-bit mode)
    #[serde(default)]
    cc: u8,

    /// MIDI channel to listen on (1–16, leave unset for omni/all channels)
    channel: Option<u8>,

    /// Smoothing time in milliseconds (0 = instant)
    #[serde(default)]
    smoothing_ms: f32,

    /// Enable 14-bit high-resolution CC mode (CC 0-31 MSB + CC 32-63 LSB)
    #[serde(default)]
    high_resolution: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MidiCcOutputs {
    #[output("output", "CC value as voltage (0-5V)", default, range = (0.0, 5.0))]
    output: f32,
}

/// State for the MidiCc module.
#[derive(Default)]
struct MidiCcState {
    sample_rate: f32,
    /// Current CC value (normalized 0.0-1.0, supports both 7-bit and 14-bit)
    current_value: f32,
    /// Smoothed output value
    smoothed_value: f32,
}

/// Converts a MIDI continuous controller (CC) message into a smooth control voltage (0–5V).
///
/// Use the `cc` parameter to select which CC number to listen to (0–127).
/// Optional `smoothingMs` applies a slew filter to reduce stepping artifacts.
/// Enable `highResolution` for 14-bit CC precision (pairs CC 0–31 with 32–63).
///
/// ## Example
///
/// ```js
/// // Use mod wheel (CC 1) to sweep a filter cutoff
/// const mod = $midiCC({ cc: 1 });
/// $lpf($saw('c3'), mod.range('c2', 'c6')).out();
///
/// // High-resolution breath controller with smoothing
/// const breath = $midiCC({ cc: 2, highResolution: true, smoothingMs: 10 });
/// $sine('c4').amplitude(breath).out();
/// ```
#[module(name = "$midiCC", args())]
pub struct MidiCc {
    outputs: MidiCcOutputs,
    params: MidiCcParams,
    state: MidiCcState,
}

impl MidiCc {
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

    /// Handle 7-bit MIDI CC message
    fn on_midi_cc(&mut self, msg: &MidiControlChange) -> Result<()> {
        // Skip 7-bit messages if in high-resolution mode (we'll use 14-bit instead)
        if self.params.high_resolution {
            return Ok(());
        }

        if msg.cc == self.params.cc
            && self.should_process_device(msg.device.as_ref())
            && self.should_process_channel(msg.channel)
        {
            // Normalize 7-bit value (0-127) to 0.0-1.0
            self.state.current_value = msg.value as f32 / 127.0;
        }
        Ok(())
    }

    /// Handle 14-bit MIDI CC message
    fn on_midi_cc_14bit(&mut self, msg: &MidiControlChange14Bit) -> Result<()> {
        // Only process if in high-resolution mode
        if !self.params.high_resolution {
            return Ok(());
        }

        if msg.cc == self.params.cc
            && self.should_process_device(msg.device.as_ref())
            && self.should_process_channel(msg.channel)
        {
            // Normalize 14-bit value (0-16383) to 0.0-1.0
            // Note: max useful value is 127*128=16256 (MSB=127, LSB=0)
            // but we normalize to full 14-bit range for simplicity
            self.state.current_value = msg.value as f32 / 16383.0;
        }
        Ok(())
    }

    fn update(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;

        // Calculate target voltage from normalized value
        let target = self.state.current_value * 5.0;

        // Apply smoothing
        if self.params.smoothing_ms > 0.0 {
            let smoothing_samples = self.params.smoothing_ms * sample_rate / 1000.0;
            let alpha = 1.0 / smoothing_samples.max(1.0);
            self.state.smoothed_value += (target - self.state.smoothed_value) * alpha;
        } else {
            self.state.smoothed_value = target;
        }

        self.outputs.output = self.state.smoothed_value;
    }
}

message_handlers!(impl MidiCc {
    MidiCC(m) => MidiCc::on_midi_cc,
    MidiCC14Bit(m) => MidiCc::on_midi_cc_14bit,
});
