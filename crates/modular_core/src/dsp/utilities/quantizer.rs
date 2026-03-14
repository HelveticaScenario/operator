//! Quantizer module - snaps input voltage to scale degrees.
//!
//! The Quantizer takes a V/Oct input signal and snaps it to the nearest note
//! in a configurable scale. This is useful for constraining melodies to a key
//! or for adding harmonic structure to random/noise sources.

use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

use crate::{
    dsp::utils::{min_gate_samples, TempGate, TempGateState},
    poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS},
    types::Connect,
    Patch,
};

use super::scale::{validate_scale_type, FixedRoot, ScaleSnapper};

/// Hysteresis amount in V/Oct (~10 cents).
/// Once a note is selected, the input must overshoot the snap boundary by this
/// amount before the quantizer will transition to a new note. Prevents rapid
/// toggling when the input hovers near a boundary.
const HYSTERESIS_VOCT: f64 = 10.0 / 1200.0;

/// Scale parameter that parses scale notation.
///
/// Supports formats:
/// - `"chromatic"` - passes through all notes unchanged
/// - `"C(major)"` - C major scale (root + scale type)
/// - `"C#(minor)"` - C# minor scale
/// - `"D(0 2 4 5 7 9 11)"` - D with custom intervals (semitones from root)
///
/// Octave syntax (e.g. `"C3(major)"`) is **not** supported here.
/// Use [`parse_with_octave`] for contexts that need an octave-aware root.
#[derive(Clone, Debug)]
pub struct ScaleParam {
    snapper: Option<Arc<ScaleSnapper>>,
    source: String,
    /// Base MIDI note for degree 0 (default 60 = C4).
    /// Computed from the root note + optional octave (when using parse_with_octave).
    base_midi: i32,
}

impl Connect for ScaleParam {
    fn connect(&mut self, _patch: &Patch) {
        // ScaleParam has no signals to connect
    }
}

impl Default for ScaleParam {
    fn default() -> Self {
        Self {
            snapper: None,
            source: String::new(),
            base_midi: 60,
        }
    }
}

impl ScaleParam {
    /// Parse a scale specification string. Octaves in the root are **not** allowed;
    /// use [`parse_with_octave`] for contexts that support them.
    pub fn parse(source: &str) -> Option<Self> {
        let result = Self::parse_inner(source, false)?;
        Some(result)
    }

    /// Parse a scale specification string, allowing an optional octave in the
    /// root (e.g. `"C3(major)"`, `"Db3(min)"`).
    pub fn parse_with_octave(source: &str) -> Option<Self> {
        Self::parse_inner(source, true)
    }

    /// Shared parse implementation.
    fn parse_inner(source: &str, allow_octave: bool) -> Option<Self> {
        let source = source.trim();

        if source.is_empty() {
            return Some(Self {
                snapper: None,
                source: source.to_string(),
                base_midi: 60,
            });
        }

        // Handle "chromatic" specially
        if source.to_lowercase() == "chromatic" {
            let root = FixedRoot::new('c', None);
            let snapper = ScaleSnapper::new(&root, "chromatic")?;
            return Some(Self {
                snapper: Some(Arc::new(snapper)),
                source: source.to_string(),
                base_midi: 60,
            });
        }

        // Parse "root(scale_type)" or "root(intervals)"
        let open_paren = source.find('(')?;
        let close_paren = source.rfind(')')?;

        if close_paren <= open_paren {
            return None;
        }

        let root_str = &source[..open_paren];
        let scale_spec = &source[open_paren + 1..close_paren];

        let root = FixedRoot::parse(root_str)?;

        // Reject octave when not allowed
        if !allow_octave && root.octave.is_some() {
            return None;
        }

        let base_midi = root.base_midi();

        // Check if scale_spec is a known scale type or custom intervals
        let snapper = if is_known_scale_type(scale_spec) {
            ScaleSnapper::new(&root, scale_spec)?
        } else {
            // Try to parse as space-separated intervals
            let intervals: Option<Vec<i8>> = scale_spec
                .split_whitespace()
                .map(|s| s.parse::<i8>().ok())
                .collect();

            let intervals = intervals?;
            if intervals.is_empty() {
                return None;
            }

            ScaleSnapper::from_intervals(&root, &intervals)
        };

        Some(Self {
            snapper: Some(Arc::new(snapper)),
            source: source.to_string(),
            base_midi,
        })
    }

    /// Get the scale snapper, if configured.
    pub fn snapper(&self) -> Option<&ScaleSnapper> {
        self.snapper.as_deref()
    }

    /// Get the base MIDI note for degree 0.
    ///
    /// When an octave is specified via [`parse_with_octave`] (e.g. "C3(major)"),
    /// this returns the MIDI note for that root+octave (e.g. 48 for C3).
    /// Without an octave, defaults to octave 4 (MIDI 60 for C).
    pub fn base_midi(&self) -> i32 {
        self.base_midi
    }
}

/// Check if a string is a known scale type name.
fn is_known_scale_type(name: &str) -> bool {
    validate_scale_type(name)
}

impl schemars::JsonSchema for ScaleParam {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ScaleParam")
    }

    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // ScaleParam is serialized as a string
        String::json_schema(_gen)
    }
}

impl<'de> Deserialize<'de> for ScaleParam {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        Self::parse(&source).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid scale specification: {}", source))
        })
    }
}

fn default_scale() -> ScaleParam {
    ScaleParam::parse("chromatic").unwrap()
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct QuantizerParams {
    /// Input V/Oct signal to quantize
    #[signal(type = pitch)]
    input: PolySignal,
    /// Offset added to input before quantization (in V/Oct)
    #[signal(type = pitch)]
    offset: Option<PolySignal>,
    /// Scale specification: "chromatic", "C(major)", "D(0 2 4 5 7 9 11)"
    #[serde(default = "default_scale")]
    scale: ScaleParam,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct QuantizerOutputs {
    #[output("output", "quantized V/Oct output", default)]
    output: PolyOutput,
    #[output("trig", "trigger pulse on note change", range = (0.0, 5.0))]
    trig: PolyOutput,
}

/// Per-channel state for tracking note changes.
#[derive(Clone, Copy, Default)]
struct ChannelState {
    /// Previous quantized voltage (None if first sample)
    prev_quantized: Option<f64>,
    /// Trigger generator for this channel
    trigger: TempGate,
}

/// State for the Quantizer module.
pub struct QuantizerState {
    /// Per-channel state for tracking note changes
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

impl Default for QuantizerState {
    fn default() -> Self {
        Self {
            channels: std::array::from_fn(|_| ChannelState {
                prev_quantized: None,
                trigger: TempGate::new_gate(TempGateState::Low),
            }),
        }
    }
}

/// Snaps a V/Oct signal to the nearest note in a given scale.
///
/// Feed any continuous pitch signal into **input** and choose a **scale** —
/// the output locks to the closest scale degree. A **trig** pulse fires
/// whenever the quantized note changes, useful for re-triggering envelopes.
///
/// Scale format examples:
/// - `"chromatic"` — all 12 semitones
/// - `"C(major)"` — C major scale
/// - `"C#(minor)"` — C# minor scale
/// - `"D(0 2 4 5 7 9 11)"` — custom intervals from root
///
/// ```js
/// // quantize a random signal to C major
/// $sine($quantizer($sine(".1hz").range(0,3), "C(major)"))
/// ```
#[module(name = "$quantizer", args(input, scale))]
pub struct Quantizer {
    outputs: QuantizerOutputs,
    params: QuantizerParams,
    state: QuantizerState,
}

impl Quantizer {
    pub fn update(&mut self, sample_rate: f32) {
        let num_channels = self.params.input.channels();
        let hold = min_gate_samples(sample_rate);

        for ch in 0..num_channels {
            let input = self.params.input.get_value(ch) as f64;
            let offset = self.params.offset.value_or_zero(ch) as f64;

            let combined = input + offset;

            let quantized = if let Some(snapper) = self.params.scale.snapper() {
                let raw_quantized = snapper.snap_voct(combined);
                let state = &self.state.channels[ch];

                // Apply hysteresis: only change note if input overshoots the
                // snap boundary by at least HYSTERESIS_VOCT.
                if let Some(prev) = state.prev_quantized {
                    if (raw_quantized - prev).abs() > 1e-6 {
                        // The raw snap wants a different note — re-snap with a
                        // small bias toward the current note to see if the
                        // input has truly crossed the threshold.
                        let bias = if raw_quantized > prev {
                            -HYSTERESIS_VOCT
                        } else {
                            HYSTERESIS_VOCT
                        };
                        let biased = snapper.snap_voct(combined + bias);
                        if (biased - prev).abs() > 1e-6 {
                            raw_quantized // even with bias, still new note → accept
                        } else {
                            prev // bias pulls it back → stay on current note
                        }
                    } else {
                        raw_quantized // same note, no change
                    }
                } else {
                    raw_quantized // first sample
                }
            } else {
                // No scale configured, pass through
                combined
            };

            // Check if the note changed
            let state = &mut self.state.channels[ch];
            let note_changed = match state.prev_quantized {
                Some(prev) => (quantized - prev).abs() > 1e-6,
                None => true, // First sample counts as a change
            };
            state.prev_quantized = Some(quantized);

            // Set gate and trigger on note change
            if note_changed {
                state
                    .trigger
                    .set_state(TempGateState::High, TempGateState::Low, hold);
            }

            self.outputs.output.set(ch, quantized as f32);
            self.outputs.trig.set(ch, state.trigger.process());
        }
    }
}

message_handlers!(impl Quantizer {});

#[cfg(test)]
mod tests {
    use crate::dsp::utils::{GATE_HIGH_VOLTAGE, GATE_LOW_VOLTAGE};

    use super::*;

    #[test]
    fn test_scale_param_parse_chromatic() {
        let scale = ScaleParam::parse("chromatic").unwrap();
        assert!(scale.snapper().is_some());
    }

    #[test]
    fn test_scale_param_parse_major() {
        let scale = ScaleParam::parse("C(major)").unwrap();
        assert!(scale.snapper().is_some());
    }

    #[test]
    fn test_scale_param_parse_minor_sharp() {
        let scale = ScaleParam::parse("C#(minor)").unwrap();
        assert!(scale.snapper().is_some());
    }

    #[test]
    fn test_scale_param_parse_custom_intervals() {
        let scale = ScaleParam::parse("D(0 2 4 5 7 9 11)").unwrap();
        assert!(scale.snapper().is_some());
    }

    #[test]
    fn test_scale_param_parse_empty() {
        let scale = ScaleParam::parse("").unwrap();
        assert!(scale.snapper().is_none());
    }

    #[test]
    fn test_scale_param_parse_rejects_octave() {
        assert!(ScaleParam::parse("C3(major)").is_none());
        assert!(ScaleParam::parse("Db3(min)").is_none());
    }

    #[test]
    fn test_scale_param_parse_with_octave() {
        let scale = ScaleParam::parse_with_octave("C3(major)").unwrap();
        assert_eq!(scale.base_midi(), 48);
        assert!(scale.snapper().is_some());

        let scale = ScaleParam::parse_with_octave("Db3(min)").unwrap();
        assert_eq!(scale.base_midi(), 49);

        // Without octave still works and defaults to 4
        let scale = ScaleParam::parse_with_octave("C(major)").unwrap();
        assert_eq!(scale.base_midi(), 60);
    }

    #[test]
    fn test_scale_param_quantize_c_major() {
        let scale = ScaleParam::parse("C(major)").unwrap();
        let snapper = scale.snapper().unwrap();

        // C4 = MIDI 60 = V/Oct 0.0, should stay C
        let c4_voct = (60.0 - 60.0) / 12.0;
        let snapped = snapper.snap_voct(c4_voct);
        assert!((snapped - c4_voct).abs() < 0.001);

        // C#4 = MIDI 61 = V/Oct 0.0833, should snap to C
        let cs4_voct = (61.0 - 60.0) / 12.0;
        let snapped = snapper.snap_voct(cs4_voct);
        assert!((snapped - c4_voct).abs() < 0.001);
    }

    #[test]
    fn test_channel_state_note_change_detection() {
        // Test the note change detection logic directly
        let mut state = ChannelState {
            prev_quantized: None,
            trigger: TempGate::new_gate(TempGateState::Low),
        };

        // First sample - should detect change (None -> Some)
        let note_changed = match state.prev_quantized {
            Some(prev) => (0.0_f64 - prev).abs() > 1e-6,
            None => true,
        };
        assert!(note_changed, "first sample should count as change");
        state.prev_quantized = Some(0.0);

        // Second sample, same note - should NOT detect change
        let note_changed = match state.prev_quantized {
            Some(prev) => (0.0_f64 - prev).abs() > 1e-6,
            None => true,
        };
        assert!(!note_changed, "same note should not trigger change");

        // Third sample, different note - should detect change
        let note_changed = match state.prev_quantized {
            Some(prev) => (1.0_f64 / 12.0 - prev).abs() > 1e-6,
            None => true,
        };
        assert!(note_changed, "different note should trigger change");
    }

    #[test]
    fn test_temp_gate_trigger_behavior() {
        // Test that TempGate produces correct multi-sample pulse
        let mut trigger = TempGate::new_gate(TempGateState::Low);

        // Initially low
        assert_eq!(trigger.process(), GATE_LOW_VOLTAGE);

        // Trigger a pulse (High then Low) with hold of 1 sample
        trigger.set_state(TempGateState::High, TempGateState::Low, 1);
        assert_eq!(
            trigger.process(),
            GATE_HIGH_VOLTAGE,
            "should be high on first process after trigger"
        );
        assert_eq!(
            trigger.process(),
            GATE_LOW_VOLTAGE,
            "should return to low on second process"
        );
        assert_eq!(trigger.process(), GATE_LOW_VOLTAGE, "should stay low");

        // Trigger with hold of 3 samples
        trigger.set_state(TempGateState::High, TempGateState::Low, 3);
        assert_eq!(trigger.process(), GATE_HIGH_VOLTAGE, "hold sample 1");
        assert_eq!(trigger.process(), GATE_HIGH_VOLTAGE, "hold sample 2");
        assert_eq!(trigger.process(), GATE_HIGH_VOLTAGE, "hold sample 3");
        assert_eq!(
            trigger.process(),
            GATE_LOW_VOLTAGE,
            "should be low after hold expires"
        );
    }

    #[test]
    fn test_snap_voct_output_is_clean_step() {
        // Sweeping input within a single semitone should produce a constant output
        let scale = ScaleParam::parse("C(major)").unwrap();
        let snapper = scale.snapper().unwrap();

        let c4_voct = 0.0; // C4
                           // Sweep from C4 to ~C4 + 49 cents — all should snap to exactly C4
        for i in 0..50 {
            let input = c4_voct + (i as f64 * 0.01) / 12.0; // fractional semitone in V/Oct
            let snapped = snapper.snap_voct(input);
            assert!(
                (snapped - c4_voct).abs() < 1e-9,
                "input V/Oct {input} should snap to C4 (0.0), got {snapped}"
            );
        }
    }

    #[test]
    fn test_hysteresis_prevents_boundary_chatter() {
        // Simulate the quantizer's hysteresis logic at the C/D boundary in C major.
        // C4 = 0.0 V/Oct,  D4 = 2/12 V/Oct ≈ 0.16667
        // Midpoint ≈ 1/12 ≈ 0.08333 V/Oct  (MIDI 61, C#, snaps to C or D)
        // With hysteresis the note should not flip until input overshoots by HYSTERESIS_VOCT.

        let scale = ScaleParam::parse("C(major)").unwrap();
        let snapper = scale.snapper().unwrap();

        let c4: f64 = 0.0;
        let d4: f64 = 2.0 / 12.0;

        // Boundary sits at 1.5 semitones above C in V/Oct = 1.5/12
        let boundary = 1.5 / 12.0;

        // Simulate: start on C4, slowly increase toward D4
        let mut prev_quantized: f64 = c4;

        // Just below boundary + hysteresis → should stay on C
        let input_just_below = boundary + HYSTERESIS_VOCT * 0.5;
        let raw = snapper.snap_voct(input_just_below);
        let quantized = if (raw - prev_quantized).abs() > 1e-6 {
            let bias = if raw > prev_quantized {
                -HYSTERESIS_VOCT
            } else {
                HYSTERESIS_VOCT
            };
            let biased = snapper.snap_voct(input_just_below + bias);
            if (biased - prev_quantized).abs() > 1e-6 {
                raw
            } else {
                prev_quantized
            }
        } else {
            raw
        };
        assert!(
            (quantized - c4).abs() < 1e-6,
            "near boundary should stay on C4 due to hysteresis, got {quantized}"
        );

        // Well past boundary + hysteresis → should switch to D
        prev_quantized = c4;
        let input_well_past = boundary + HYSTERESIS_VOCT * 3.0;
        let raw = snapper.snap_voct(input_well_past);
        let quantized = if (raw - prev_quantized).abs() > 1e-6 {
            let bias = if raw > prev_quantized {
                -HYSTERESIS_VOCT
            } else {
                HYSTERESIS_VOCT
            };
            let biased = snapper.snap_voct(input_well_past + bias);
            if (biased - prev_quantized).abs() > 1e-6 {
                raw
            } else {
                prev_quantized
            }
        } else {
            raw
        };
        assert!(
            (quantized - d4).abs() < 1e-6,
            "well past boundary should switch to D4, got {quantized}"
        );
    }
}
