//! Polyphonic signal support for multichannel cables.
//!
//! This module provides VCV Rack-style polyphonic signal handling,
//! allowing a single cable to carry up to 16 independent audio channels.
//!
//! - `PolyOutput`: A fixed-capacity output buffer with channel count metadata (for module outputs)
//! - `PolySignal`: A fixed-capacity input buffer containing Signal values (for polyphonic module inputs)

use crate::{dsp::utils::sanitize, types::Signal};
use deserr::{DeserializeError, ErrorKind, IntoValue, ValuePointerRef};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;

/// Maximum channels per cable. A hard cap on polyphony — buffers are sized to
/// each module's actual channel count, so this only bounds the maximum, it is
/// not allocated up front.
pub const PORT_MAX_CHANNELS: usize = 64;

/// A polyphonic output buffer holding one voltage per active channel.
///
/// The backing slice is sized to the module's channel count (1..=16) on the
/// main thread at construction (`set_channels`), never on the audio thread.
/// `voltages.len()` is the active channel count: 0 = disconnected, 1 = mono,
/// 2-16 = polyphonic.
#[derive(Clone, Debug, PartialEq)]
pub struct PolyOutput {
    /// One voltage per active channel. Length is the channel count.
    voltages: Box<[f32]>,
}

impl Default for PolyOutput {
    fn default() -> Self {
        Self {
            voltages: Box::new([]), // Disconnected (0 channels)
        }
    }
}

impl PolyOutput {
    /// Create a monophonic signal with a single value
    pub fn mono(value: f32) -> Self {
        Self {
            voltages: vec![value].into_boxed_slice(),
        }
    }

    // === Accessors ===

    /// Get voltage for a specific channel (returns 0.0 if out of range)
    pub fn get(&self, channel: usize) -> f32 {
        self.voltages.get(channel).copied().unwrap_or(0.0)
    }

    /// Set voltage for a specific channel
    pub fn set(&mut self, channel: usize, value: f32) {
        if let Some(slot) = self.voltages.get_mut(channel) {
            *slot = sanitize(value);
        }
    }

    /// Get voltage with modulo cycling: channel wraps around available channels.
    /// This is consistent with Vec::cycle_get for non-signal params.
    /// A mono signal cycles to all channels, a 2-ch signal alternates, etc.
    pub fn get_cycling(&self, channel: usize) -> f32 {
        let n = self.voltages.len();
        if n == 0 {
            0.0 // Disconnected
        } else {
            self.voltages[channel % n]
        }
    }

    /// Size the buffer to `channels` active channels.
    ///
    /// Reallocates when the count changes — **main thread only** (called once
    /// per output at construction via `OutputStruct::set_all_channels`). When
    /// the count is unchanged this is a no-op, so it is safe to re-invoke.
    pub fn set_channels(&mut self, channels: usize) {
        let channels = channels.min(PORT_MAX_CHANNELS);
        if self.voltages.len() != channels {
            self.voltages = vec![0.0; channels].into_boxed_slice();
        }
    }

    pub fn channels(&self) -> usize {
        self.voltages.len()
    }
}

// === Serialization ===

impl Serialize for PolyOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as a struct with channels and voltages array
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("PolyOutput", 2)?;
        state.serialize_field("channels", &self.voltages.len())?;
        state.serialize_field("voltages", &self.voltages[..])?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for PolyOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PolyOutputDe {
            channels: usize,
            voltages: Vec<f32>,
        }

        let de = PolyOutputDe::deserialize(deserializer)?;
        let channels = de.channels.min(PORT_MAX_CHANNELS);
        let mut voltages = vec![0.0f32; channels].into_boxed_slice();
        for (i, &v) in de.voltages.iter().enumerate().take(channels) {
            voltages[i] = v;
        }
        Ok(PolyOutput { voltages })
    }
}

// === JsonSchema ===

impl JsonSchema for PolyOutput {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PolyOutput")
    }

    fn json_schema(r#gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Schema matches the serialized form
        #[derive(JsonSchema)]
        #[allow(dead_code)]
        struct PolyOutputSchema {
            channels: usize,
            voltages: Vec<f32>,
        }
        PolyOutputSchema::json_schema(r#gen)
    }
}

// =============================================================================
// PolySignal - Polyphonic input containing multiple Signal values
// =============================================================================

/// A polyphonic input buffer containing multiple Signal values.
///
/// This is used for module inputs that need to accept polyphonic connections.
/// Each channel is a separate Signal (Volts or Cable).
/// A PolySignal always has at least 1 channel:
/// - 1 = monophonic (single signal)
/// - 2-16 = polyphonic (multiple signals)
///
/// Disconnected inputs are represented as `Option<PolySignal>` at the param level.
#[derive(Clone, Debug)]
pub struct PolySignal {
    /// Active signal channels (always at least 1, up to PORT_MAX_CHANNELS).
    /// Sized to the actual channel count on the main thread at deserialize.
    channels: Box<[Signal]>,
}

// Hand-written rather than `#[derive(Connect)]`: deriving would require a
// `Connect for Box<[Signal]>` impl, which overlaps the blanket `Connect for
// Box<T>` under coherence. Forwarding over the slice directly sidesteps that.
impl crate::types::Connect for PolySignal {
    fn connect(&mut self, patch: &crate::Patch) {
        for signal in self.channels.iter_mut() {
            signal.connect(patch);
        }
    }
    fn collect_cables(&self, sink: &mut Vec<String>) {
        for signal in self.channels.iter() {
            signal.collect_cables(sink);
        }
    }
    fn inject_index_ptr(&mut self, ptr: *const std::cell::Cell<usize>) {
        for signal in self.channels.iter_mut() {
            signal.inject_index_ptr(ptr);
        }
    }
}

impl PolySignal {
    /// Create a mono (1-channel) PolySignal from a single Signal.
    pub fn mono(signal: Signal) -> Self {
        Self {
            channels: vec![signal].into_boxed_slice(),
        }
    }

    /// Create a polyphonic PolySignal from a slice of Signals.
    pub fn poly(signals: &[Signal]) -> Self {
        assert!(
            !signals.is_empty(),
            "PolySignal must have at least 1 channel"
        );
        let channels = signals
            .iter()
            .take(PORT_MAX_CHANNELS)
            .cloned()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self { channels }
    }

    // === Accessors ===

    /// Get the number of active channels
    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    /// Check if monophonic (exactly 1 channel)
    pub fn is_monophonic(&self) -> bool {
        self.channels.len() == 1
    }

    /// Check if polyphonic (more than 1 channel)
    pub fn is_polyphonic(&self) -> bool {
        self.channels.len() > 1
    }

    /// Get signal at a specific channel (returns None if out of range)
    pub fn get(&self, channel: usize) -> Option<&Signal> {
        self.channels.get(channel)
    }

    /// Get signal with cycling (wraps around available channels)
    pub fn get_cycling(&self, channel: usize) -> &Signal {
        &self.channels[channel % self.channels.len()]
    }

    /// Get the f32 value at a channel with cycling
    pub fn get_value(&self, channel: usize) -> f32 {
        self.channels[channel % self.channels.len()].get_value()
    }

    /// Calculate the maximum channel count across multiple PolySignals
    pub fn max_channels(poly_signals: &[&PolySignal]) -> usize {
        poly_signals
            .iter()
            .map(|sig| sig.channels.len())
            .max()
            .unwrap_or(0)
    }
}

impl From<MonoSignal> for PolySignal {
    fn from(mono: MonoSignal) -> PolySignal {
        mono.inner
    }
}

// === Serialization for PolySignal ===

impl Serialize for PolySignal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as array of active signals
        let signals: Vec<&Signal> = self.channels.iter().collect();
        signals.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PolySignal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Accept either a single signal or an array of signals
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PolySignalDe {
            Single(Signal),
            Array(Vec<Signal>),
        }

        match PolySignalDe::deserialize(deserializer)? {
            PolySignalDe::Single(signal) => Ok(PolySignal::mono(signal)),
            PolySignalDe::Array(signals) => {
                if signals.is_empty() {
                    return Err(serde::de::Error::custom(
                        "PolySignal must have at least 1 channel",
                    ));
                }
                if signals.len() > PORT_MAX_CHANNELS {
                    return Err(serde::de::Error::custom(format!(
                        "PolySignal cannot exceed {} channels",
                        PORT_MAX_CHANNELS
                    )));
                }
                Ok(PolySignal::poly(&signals))
            }
        }
    }
}

// deserr implementation for PolySignal - accepts single Signal or array of Signals.
impl<E: DeserializeError> deserr::Deserr<E> for PolySignal {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        match &value {
            deserr::Value::Sequence(_) => {
                // Array of signals
                let signals =
                    <Vec<crate::types::Signal> as deserr::Deserr<E>>::deserialize_from_value(
                        value, location,
                    )?;
                if signals.is_empty() {
                    return Err(deserr::take_cf_content(E::error::<V>(
                        None,
                        ErrorKind::Unexpected {
                            msg: "PolySignal must have at least 1 channel".to_string(),
                        },
                        location,
                    )));
                }
                if signals.len() > PORT_MAX_CHANNELS {
                    return Err(deserr::take_cf_content(E::error::<V>(
                        None,
                        ErrorKind::Unexpected {
                            msg: format!("PolySignal cannot exceed {} channels", PORT_MAX_CHANNELS),
                        },
                        location,
                    )));
                }
                Ok(PolySignal::poly(&signals))
            }
            _ => {
                // Single signal
                let signal = <crate::types::Signal as deserr::Deserr<E>>::deserialize_from_value(
                    value, location,
                )?;
                Ok(PolySignal::mono(signal))
            }
        }
    }
}

impl JsonSchema for PolySignal {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PolySignal")
    }

    fn json_schema(r#gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Schema: either a single Signal or array of Signals
        #[derive(JsonSchema)]
        #[serde(untagged)]
        #[allow(dead_code)]
        enum PolySignalSchema {
            Single(crate::types::Signal),
            Array(Vec<crate::types::Signal>),
        }
        PolySignalSchema::json_schema(r#gen)
    }
}

// =============================================================================
// MonoSignal - Polyphonic input that sums to mono output
// =============================================================================

/// A polyphonic input that sums all channels to a single mono value.
///
/// This wraps a `PolySignal` but provides a `Signal`-like API where
/// `get_value()` returns the sum of all channels in the PolySignal.
///
/// Use this for module inputs that accept polyphonic connections but produce
/// a mono control signal (e.g., stereo width, etc.).
///
/// Disconnected inputs are represented as `Option<MonoSignal>` at the param level.
///
/// For polyphony propagation, MonoSignal is treated as a single channel.
#[derive(Clone, Debug, Connect)]
pub struct MonoSignal {
    inner: PolySignal,
}

impl MonoSignal {
    /// Create a MonoSignal from a PolySignal
    pub fn from_poly(poly: PolySignal) -> Self {
        Self { inner: poly }
    }

    /// Get the summed value of all channels
    pub fn get_value(&self) -> f32 {
        self.inner.channels.iter().map(|s| s.get_value()).sum()
    }

    /// Get the summed value of all channels as f64
    pub fn get_value_f64(&self) -> f64 {
        self.inner
            .channels
            .iter()
            .map(|ch| ch.get_value() as f64)
            .sum::<f64>()
    }
}

impl From<PolySignal> for MonoSignal {
    fn from(poly: PolySignal) -> MonoSignal {
        MonoSignal { inner: poly }
    }
}

// === Serialization for MonoSignal (delegates to PolySignal) ===

impl Serialize for MonoSignal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MonoSignal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(MonoSignal {
            inner: PolySignal::deserialize(deserializer)?,
        })
    }
}

// deserr implementation for MonoSignal - delegates to PolySignal.
impl<E: DeserializeError> deserr::Deserr<E> for MonoSignal {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        Ok(MonoSignal {
            inner: PolySignal::deserialize_from_value(value, location)?,
        })
    }
}

impl JsonSchema for MonoSignal {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MonoSignal")
    }

    fn json_schema(r#gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Reuse PolySignal's schema
        PolySignal::json_schema(r#gen)
    }
}

// =============================================================================
// Extension traits for Option<PolySignal> and Option<MonoSignal>
// =============================================================================

/// Extension trait for `Option<PolySignal>` providing normalled/default value access.
///
/// This replaces the old `is_disconnected()` and `get_value_or()` methods that were
/// on `PolySignal` itself. Disconnection is now represented by `None` at the param level.
pub trait PolySignalExt {
    /// Returns the signal's value at the given channel (cycling), or `default` if disconnected.
    fn value_or(&self, ch: usize, default: f32) -> f32;

    /// Returns the signal's value at the given channel (cycling), or 0.0 if disconnected.
    fn value_or_zero(&self, ch: usize) -> f32;

    /// Returns the number of active channels, or 0 if disconnected.
    fn channel_count(&self) -> usize;

    /// Returns true if the signal is disconnected (None).
    fn is_disconnected(&self) -> bool;
}

impl PolySignalExt for Option<PolySignal> {
    fn value_or(&self, ch: usize, default: f32) -> f32 {
        match self {
            Some(ps) => ps.get_value(ch),
            None => default,
        }
    }

    fn value_or_zero(&self, ch: usize) -> f32 {
        self.value_or(ch, 0.0)
    }

    fn channel_count(&self) -> usize {
        match self {
            Some(ps) => ps.channels(),
            None => 0,
        }
    }

    fn is_disconnected(&self) -> bool {
        self.is_none()
    }
}

/// Extension trait for `Option<MonoSignal>` providing normalled/default value access.
///
/// This replaces the old `is_disconnected()` and `get_value_or()` methods that were
/// on `MonoSignal` itself. Disconnection is now represented by `None` at the param level.
pub trait MonoSignalExt {
    /// Returns the summed signal value, or `default` if disconnected.
    fn value_or(&self, default: f32) -> f32;

    /// Returns the summed signal value, or 0.0 if disconnected.
    fn value_or_zero(&self) -> f32;

    /// Returns true if the signal is disconnected (None).
    fn is_disconnected(&self) -> bool;
}

impl MonoSignalExt for Option<MonoSignal> {
    fn value_or(&self, default: f32) -> f32 {
        match self {
            Some(ms) => ms.get_value(),
            None => default,
        }
    }

    fn value_or_zero(&self) -> f32 {
        self.value_or(0.0)
    }

    fn is_disconnected(&self) -> bool {
        self.is_none()
    }
}

#[cfg(test)]
mod tests {
    use crate::dsp::utils::hz_to_voct;

    use super::*;

    #[test]
    fn test_poly_signal_mono() {
        let sig = PolySignal::mono(Signal::Volts(1.0));
        assert_eq!(sig.channels(), 1);
        assert!(sig.is_monophonic());
        assert!(!sig.is_polyphonic());
        assert_eq!(sig.get_value(0), 1.0);
    }

    #[test]
    fn test_poly_signal_poly() {
        let sig = PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(2.0), Signal::Volts(3.0)]);
        assert_eq!(sig.channels(), 3);
        assert!(!sig.is_monophonic());
        assert!(sig.is_polyphonic());
        assert_eq!(sig.get_value(0), 1.0);
        assert_eq!(sig.get_value(1), 2.0);
        assert_eq!(sig.get_value(2), 3.0);
        // Cycling: channel 3 wraps to channel 0
        assert_eq!(sig.get_value(3), 1.0);
    }

    #[test]
    fn test_poly_signal_get_returns_option() {
        let sig = PolySignal::mono(Signal::Volts(1.0));
        assert!(sig.get(0).is_some());
        assert!(sig.get(1).is_none());
    }

    #[test]
    fn test_poly_signal_deserialize_string() {
        // Deserialize "440hz" string into PolySignal
        let json = r#""440hz""#;
        let result: PolySignal = serde_json::from_str(json).expect("Failed to deserialize");
        println!("Deserialized '440hz': channels = {}", result.channels());
        assert_eq!(
            result.channels(),
            1,
            "String should deserialize to 1 channel"
        );

        let value = result.get_value(0);
        println!("Value at channel 0: {}", value);
        let target = hz_to_voct(440.0);
        assert!(
            (value - target).abs() < 0.01,
            "Value should be {} v/oct, got {}",
            target,
            value
        );
    }

    #[test]
    fn test_poly_signal_deserialize_number() {
        let json = "4.0";
        let result: PolySignal = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(
            result.channels(),
            1,
            "Number should deserialize to 1 channel"
        );
        assert_eq!(result.get_value(0), 4.0);
    }

    #[test]
    fn test_poly_signal_deserialize_empty_array_fails() {
        let json = "[]";
        let result: Result<PolySignal, _> = serde_json::from_str(json);
        assert!(result.is_err(), "Empty array should fail to deserialize");
    }

    #[test]
    fn test_poly_output() {
        let mut sig = PolyOutput::default();
        sig.set_channels(3);
        sig.set(0, 1.0);
        sig.set(1, 2.0);
        sig.set(2, 3.0);
        assert_eq!(sig.channels(), 3);
        assert_eq!(sig.get(0), 1.0);
        assert_eq!(sig.get(1), 2.0);
        assert_eq!(sig.get(2), 3.0);
        assert_eq!(sig.get(3), 0.0);
    }

    #[test]
    fn test_poly_output_cycling() {
        let mut sig = PolyOutput::default();
        sig.set_channels(2);
        sig.set(0, 1.0);
        sig.set(1, 2.0);
        assert_eq!(sig.get_cycling(0), 1.0);
        assert_eq!(sig.get_cycling(1), 2.0);
        assert_eq!(sig.get_cycling(2), 1.0); // wraps
        assert_eq!(sig.get_cycling(3), 2.0); // wraps
    }

    #[test]
    fn test_poly_output_disconnected() {
        let sig = PolyOutput::default();
        assert_eq!(sig.channels(), 0);
        assert_eq!(sig.get_cycling(0), 0.0);
    }

    #[test]
    fn test_deserialize_poly() {
        use serde_json::from_str;

        // Single value
        let v: Vec<String> = from_str(r#""pink""#).map(|s: String| vec![s]).unwrap();
        assert_eq!(v, vec!["pink"]);

        // Array
        let v: Vec<String> = from_str(r#"["white", "pink", "brown"]"#).unwrap();
        assert_eq!(v, vec!["white", "pink", "brown"]);
    }

    #[test]
    fn test_mono_signal_single_channel() {
        let json = "2.5";
        let mono: MonoSignal = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(mono.get_value(), 2.5);
    }

    #[test]
    fn test_mono_signal_sums_channels() {
        // MonoSignal should sum all channels
        let json = "[1.0, 2.0, 3.0]";
        let mono: MonoSignal = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(mono.get_value(), 6.0); // 1 + 2 + 3 = 6
    }

    // === Extension trait tests ===

    #[test]
    fn test_poly_signal_ext_none() {
        let opt: Option<PolySignal> = None;
        assert!(opt.is_disconnected());
        assert_eq!(opt.channel_count(), 0);
        assert_eq!(opt.value_or(0, 5.0), 5.0);
        assert_eq!(opt.value_or_zero(0), 0.0);
    }

    #[test]
    fn test_poly_signal_ext_some() {
        let opt: Option<PolySignal> = Some(PolySignal::mono(Signal::Volts(3.0)));
        assert!(!opt.is_disconnected());
        assert_eq!(opt.channel_count(), 1);
        assert_eq!(opt.value_or(0, 5.0), 3.0);
        assert_eq!(opt.value_or_zero(0), 3.0);
    }

    #[test]
    fn test_mono_signal_ext_none() {
        let opt: Option<MonoSignal> = None;
        assert!(opt.is_disconnected());
        assert_eq!(opt.value_or(5.0), 5.0);
        assert_eq!(opt.value_or_zero(), 0.0);
    }

    #[test]
    fn test_mono_signal_ext_some() {
        let mono = MonoSignal::from_poly(PolySignal::mono(Signal::Volts(2.0)));
        let opt: Option<MonoSignal> = Some(mono);
        assert!(!opt.is_disconnected());
        assert_eq!(opt.value_or(5.0), 2.0);
        assert_eq!(opt.value_or_zero(), 2.0);
    }
}
