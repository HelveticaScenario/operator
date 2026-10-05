//! Quantizer module - snaps input voltage to the notes of a scale signal.

use arrayvec::ArrayVec;
use deserr::{DeserializeError, Deserr, IntoValue, ValuePointerRef};
use schemars::JsonSchema;

use crate::{
    Patch,
    dsp::utils::{SchmittTrigger, TempGate, TempGateState, min_gate_samples},
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
    types::Connect,
};

use super::scale::ScaleSpec;

/// Hysteresis amount in V/Oct (~10 cents).
/// Once a note is selected, the input must overshoot the snap boundary by this
/// amount before the quantizer will transition to a new note. Prevents rapid
/// toggling when the input hovers near a boundary.
const HYSTERESIS_VOCT: f64 = 10.0 / 1200.0;

/// The pitches a quantizer snaps to: one V/Oct note per channel.
///
/// The engine only accepts a signal here. The DSL rewrites a spec string
/// (`"C(major)"`, `"C[maj7]"`) into a `$chord` module feeding this param, so
/// the schema advertises both forms to the editor.
#[derive(Clone)]
pub struct ScaleSignal(PolySignal);

impl Connect for ScaleSignal {
    fn apply_default_connections(&mut self) {
        self.0.apply_default_connections();
    }
    fn connect(&mut self, patch: &Patch) {
        self.0.connect(patch);
    }
    fn collect_cables(&self, sink: &mut Vec<String>) {
        self.0.collect_cables(sink);
    }
    fn inject_index_ptr(&mut self, ptr: *const std::cell::Cell<usize>) {
        self.0.inject_index_ptr(ptr);
    }
}

impl JsonSchema for ScaleSignal {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ScaleSignal")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        #[derive(JsonSchema)]
        #[serde(untagged)]
        #[allow(dead_code)]
        enum ScaleSignalSchema {
            Spec(ScaleSpec),
            Signal(PolySignal),
        }
        ScaleSignalSchema::json_schema(generator)
    }
}

impl<E: DeserializeError> deserr::Deserr<E> for ScaleSignal {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        PolySignal::deserialize_from_value(value, location).map(Self)
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct QuantizerParams {
    /// Input V/Oct signal to quantize
    #[signal(type = pitch)]
    input: PolySignal,
    /// Offset added to input before quantization (in V/Oct)
    #[signal(type = pitch)]
    #[deserr(default)]
    offset: Option<PolySignal>,
    /// Notes to snap to, one V/Oct pitch per channel (octave is ignored), or a
    /// spec string like "C(major)" / "C[maj7]". Snaps to semitones if omitted.
    #[deserr(default)]
    scale: Option<ScaleSignal>,
    /// Per-note gate for **scale**: scale channel N is used only while gate
    /// channel N is high. While every gate channel is low the output holds.
    #[signal(type = gate)]
    #[deserr(default)]
    gate: Option<PolySignal>,
}

/// As wide as `input` and `offset`. The scale and its gate describe one shared
/// set of notes, so neither widens the module.
#[allow(private_interfaces)]
pub fn quantizer_derive_channel_count(params: &QuantizerParams) -> usize {
    params
        .input
        .channels()
        .max(params.offset.channel_count())
        .max(1)
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
#[derive(Clone, Copy)]
struct ChannelState {
    /// Previous quantized voltage (None until the first note is chosen)
    prev_quantized: Option<f64>,
    /// Trigger generator for this channel
    trigger: TempGate,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            prev_quantized: None,
            trigger: TempGate::new_gate(TempGateState::Low),
        }
    }
}

struct QuantizerState {
    /// One per scale channel, reading the matching gate channel.
    gate_triggers: [SchmittTrigger; PORT_MAX_CHANNELS],
}

impl Default for QuantizerState {
    fn default() -> Self {
        Self {
            gate_triggers: [SchmittTrigger::default(); PORT_MAX_CHANNELS],
        }
    }
}

/// Snap `x` to the nearest pitch in `notes` (octave-reduced to `[0, 1)`),
/// in any octave. Ties resolve to the lower pitch.
fn snap_to_notes(x: f64, notes: &[f64]) -> f64 {
    let octave = x.floor();
    let frac = x - octave;
    let mut best = frac;
    let mut best_dist = f64::INFINITY;
    for &note in notes {
        for candidate in [note - 1.0, note, note + 1.0] {
            let dist = (candidate - frac).abs();
            if dist < best_dist || (dist == best_dist && candidate < best) {
                best = candidate;
                best_dist = dist;
            }
        }
    }
    octave + best
}

/// Snap `x` to the nearest 12-TET semitone.
fn snap_to_semitone(x: f64) -> f64 {
    (x * 12.0).round() / 12.0
}

/// Snap with hysteresis: leave `prev` only once `x` overshoots the snap
/// boundary by at least `HYSTERESIS_VOCT`.
fn snap_with_hysteresis(x: f64, prev: Option<f64>, snap: impl Fn(f64) -> f64) -> f64 {
    let raw = snap(x);
    let Some(prev) = prev else {
        return raw;
    };
    if (raw - prev).abs() <= 1e-6 {
        return raw;
    }
    // Re-snap with a small bias toward the current note to see whether the
    // input has truly crossed the threshold.
    let bias = if raw > prev {
        -HYSTERESIS_VOCT
    } else {
        HYSTERESIS_VOCT
    };
    if (snap(x + bias) - prev).abs() > 1e-6 {
        raw
    } else {
        prev
    }
}

/// Snaps a V/Oct signal to the nearest note of a scale.
///
/// **scale** is a polyphonic signal: each channel is one V/Oct note, and the
/// output snaps to the nearest of those notes in any octave. Feed it from
/// `$chord`, a `$cycle`, or `$midiCV` — or pass a spec string such as
/// `"C(major)"` or `"C[maj7]"`, which is shorthand for `$chord(spec)` (see
/// `$chord` for the full grammar). Without a scale, the output snaps to
/// semitones.
///
/// **gate** selects which scale notes are active: scale channel N is used only
/// while gate channel N is high. While every gate channel is low, the output
/// holds its last note.
///
/// A **trig** pulse fires whenever the quantized note changes, useful for
/// re-triggering envelopes.
///
/// ```js
/// // quantize a slow sine to C major
/// $sine($quantizer($sine("0.1hz").range(0, 3), "C(major)")).out()
/// ```
///
/// ```js
/// // quantize to whichever MIDI keys are held; holds when all are released
/// const midi = $midiCV({ channels: 4 })
/// $sine($quantizer($sine("0.1hz").range(-1, 1), midi, { gate: midi.gate })).out()
/// ```
#[module(name = "$quantizer", channels_derive = quantizer_derive_channel_count, args(input, scale))]
pub struct Quantizer {
    outputs: QuantizerOutputs,
    params: QuantizerParams,
    state: QuantizerState,
    channel_state: Box<[ChannelState]>,
}

impl Quantizer {
    pub fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let hold_samples = min_gate_samples(sample_rate);

        // Collect the active scale notes once; every channel snaps to the same set.
        let scale = self.params.scale.as_ref().map(|s| &s.0);
        let gate = self.params.gate.as_ref();
        let note_count = match (scale, gate) {
            (Some(scale), _) => scale.channels(),
            (None, Some(gate)) => gate.channels(),
            (None, None) => 0,
        };
        let mut notes = ArrayVec::<f64, PORT_MAX_CHANNELS>::new();
        let mut any_active = false;
        for i in 0..note_count {
            let active = match gate {
                Some(gate) => {
                    self.state.gate_triggers[i]
                        .process_with_edge(gate.get_value(i))
                        .0
                }
                None => true,
            };
            any_active |= active;
            if let (true, Some(scale)) = (active, scale) {
                let v = scale.get_value(i) as f64;
                notes.push(v - v.floor());
            }
        }
        let hold = gate.is_some() && !any_active;

        for ch in 0..num_channels {
            let input = self.params.input.get_value(ch) as f64;
            let offset = self.params.offset.value_or_zero(ch) as f64;
            let combined = input + offset;
            let state = &mut self.channel_state[ch];

            let quantized = if hold {
                // Nothing to snap to: keep the last note, or pass the input
                // through until a first note has been chosen.
                state.prev_quantized.unwrap_or(combined)
            } else {
                let quantized = match scale {
                    Some(_) => snap_with_hysteresis(combined, state.prev_quantized, |x| {
                        snap_to_notes(x, &notes)
                    }),
                    None => snap_with_hysteresis(combined, state.prev_quantized, snap_to_semitone),
                };
                let note_changed = match state.prev_quantized {
                    Some(prev) => (quantized - prev).abs() > 1e-6,
                    None => true,
                };
                if note_changed {
                    state
                        .trigger
                        .set_state(TempGateState::High, TempGateState::Low, hold_samples);
                }
                state.prev_quantized = Some(quantized);
                quantized
            };

            self.outputs.output.set(ch, quantized as f32);
            self.outputs.trig.set(ch, state.trigger.process());
        }
    }
}

message_handlers!(impl Quantizer {});

#[cfg(test)]
mod tests {
    use crate::dsp::utils::{GATE_HIGH_VOLTAGE, GATE_LOW_VOLTAGE};
    use crate::types::{OutputStruct, Signal};

    use super::*;

    fn volts(values: &[f32]) -> PolySignal {
        let signals: Vec<Signal> = values.iter().map(|&v| Signal::Volts(v)).collect();
        PolySignal::poly(&signals)
    }

    fn spec_signal(spec: &str) -> ScaleSignal {
        ScaleSignal(volts(ScaleSpec::parse(spec).unwrap().voltages()))
    }

    fn make_quantizer(params: QuantizerParams) -> Quantizer {
        let channels = quantizer_derive_channel_count(&params);
        let mut outputs = QuantizerOutputs::default();
        outputs.set_all_channels(channels);
        Quantizer {
            outputs,
            params,
            state: QuantizerState::default(),
            channel_state: vec![ChannelState::default(); channels].into_boxed_slice(),
            _channel_count: channels,
            _block_index: Default::default(),
        }
    }

    fn quantizer(input: f32, scale: Option<ScaleSignal>, gate: Option<PolySignal>) -> Quantizer {
        make_quantizer(QuantizerParams {
            input: volts(&[input]),
            offset: None,
            scale,
            gate,
        })
    }

    fn set_input(q: &mut Quantizer, v: f32) {
        q.params.input = volts(&[v]);
    }

    fn semis(q: &Quantizer) -> f32 {
        q.outputs.output.get(0) * 12.0
    }

    #[test]
    fn snaps_to_scale_notes_in_any_octave() {
        let mut q = quantizer(0.0, Some(spec_signal("C(major)")), None);
        for (input, expected) in [
            (0.0, 0.0),
            (0.9, 0.0),
            (1.1, 2.0),
            (6.4, 7.0),
            (12.9, 12.0),
            (13.2, 14.0),
            (-0.6, -1.0),
            (-12.3, -12.0),
        ] {
            q.channel_state[0] = ChannelState::default();
            set_input(&mut q, input / 12.0);
            q.update(48000.0);
            assert!(
                (semis(&q) - expected).abs() < 1e-4,
                "{input} → {}",
                semis(&q)
            );
        }
    }

    #[test]
    fn chord_notes_ignore_their_octave() {
        // A C major triad voiced across octaves still snaps every octave.
        let scale = ScaleSignal(volts(&[-1.0, 4.0 / 12.0 + 2.0, 7.0 / 12.0]));
        let mut q = quantizer(15.0 / 12.0, Some(scale), None);
        q.update(48000.0);
        assert!((semis(&q) - 16.0).abs() < 1e-4, "got {}", semis(&q));
    }

    #[test]
    fn ties_resolve_to_the_lower_note() {
        let mut q = quantizer(2.0 / 12.0, Some(spec_signal("C[0 4]")), None);
        q.update(48000.0);
        assert!(semis(&q).abs() < 1e-4, "got {}", semis(&q));
    }

    #[test]
    fn non_equal_tunings_pass_through() {
        let mut q = quantizer(4.1 / 12.0, Some(spec_signal("C(just)")), None);
        q.update(48000.0);
        let out = q.outputs.output.get(0) as f64;
        assert!((out - 1.25_f64.log2()).abs() < 1e-6, "got {out}");
    }

    #[test]
    fn without_a_scale_snaps_to_semitones() {
        let mut q = quantizer(3.4 / 12.0, None, None);
        q.update(48000.0);
        assert!((semis(&q) - 3.0).abs() < 1e-4);
    }

    #[test]
    fn gate_masks_scale_notes() {
        // C E G with only G gated on: everything snaps to G.
        let mut q = quantizer(
            0.9 / 12.0,
            Some(spec_signal("C[maj]")),
            Some(volts(&[0.0, 0.0, 5.0])),
        );
        q.update(48000.0);
        assert!((semis(&q) - (-5.0)).abs() < 1e-4, "got {}", semis(&q));
    }

    #[test]
    fn narrower_gate_cycles_across_scale_channels() {
        // Gate [high, low] over C E G → C and G active.
        let mut q = quantizer(
            5.0 / 12.0,
            Some(spec_signal("C[maj]")),
            Some(volts(&[5.0, 0.0])),
        );
        q.update(48000.0);
        assert!((semis(&q) - 7.0).abs() < 1e-4, "got {}", semis(&q));
    }

    #[test]
    fn all_gates_low_holds_without_trig() {
        let mut q = quantizer(4.0 / 12.0, Some(spec_signal("C[maj]")), Some(volts(&[5.0])));
        q.update(48000.0);
        assert!((semis(&q) - 4.0).abs() < 1e-4);
        for _ in 0..1000 {
            q.update(48000.0);
        }
        assert_eq!(q.outputs.trig.get(0), GATE_LOW_VOLTAGE);

        q.params.gate = Some(volts(&[0.0]));
        set_input(&mut q, 7.0 / 12.0);
        for _ in 0..10 {
            q.update(48000.0);
            assert!((semis(&q) - 4.0).abs() < 1e-4, "held, got {}", semis(&q));
            assert_eq!(q.outputs.trig.get(0), GATE_LOW_VOLTAGE);
        }

        q.params.gate = Some(volts(&[5.0]));
        q.update(48000.0);
        assert!((semis(&q) - 7.0).abs() < 1e-4);
        assert_eq!(q.outputs.trig.get(0), GATE_HIGH_VOLTAGE);
    }

    #[test]
    fn passes_input_through_before_any_note_is_active() {
        let mut q = quantizer(0.123, Some(spec_signal("C[maj]")), Some(volts(&[0.0])));
        q.update(48000.0);
        assert!((q.outputs.output.get(0) - 0.123).abs() < 1e-6);
        assert_eq!(q.outputs.trig.get(0), GATE_LOW_VOLTAGE);
    }

    #[test]
    fn gate_without_scale_only_holds() {
        let mut q = quantizer(3.2 / 12.0, None, Some(volts(&[5.0])));
        q.update(48000.0);
        assert!((semis(&q) - 3.0).abs() < 1e-4);
        q.params.gate = Some(volts(&[0.0]));
        set_input(&mut q, 9.0 / 12.0);
        q.update(48000.0);
        assert!((semis(&q) - 3.0).abs() < 1e-4);
    }

    #[test]
    fn changing_scale_moves_the_note() {
        let mut q = quantizer(3.0 / 12.0, Some(spec_signal("C[maj]")), None);
        q.update(48000.0);
        assert!((semis(&q) - 4.0).abs() < 1e-4);
        q.params.scale = Some(spec_signal("C[min]"));
        q.update(48000.0);
        assert!((semis(&q) - 3.0).abs() < 1e-4);
    }

    #[test]
    fn hysteresis_prevents_boundary_chatter() {
        // C/D boundary in C major sits at 1 semitone.
        let boundary = 1.0 / 12.0;
        let mut q = quantizer(0.0, Some(spec_signal("C(major)")), None);
        q.update(48000.0);
        set_input(&mut q, (boundary + HYSTERESIS_VOCT * 0.5) as f32);
        q.update(48000.0);
        assert!(
            semis(&q).abs() < 1e-4,
            "should stay on C, got {}",
            semis(&q)
        );
        set_input(&mut q, (boundary + HYSTERESIS_VOCT * 3.0) as f32);
        q.update(48000.0);
        assert!(
            (semis(&q) - 2.0).abs() < 1e-4,
            "should move to D, got {}",
            semis(&q)
        );
    }

    #[test]
    fn width_ignores_scale_and_gate() {
        let q = make_quantizer(QuantizerParams {
            input: volts(&[0.0]),
            offset: None,
            scale: Some(spec_signal("chromatic")),
            gate: Some(volts(&[5.0; 8])),
        });
        assert_eq!(q.channel_count(), 1);
    }

    #[test]
    fn offset_wider_than_input_drives_all_declared_channels() {
        let g4 = 7.0 / 12.0;
        let mut q = make_quantizer(QuantizerParams {
            input: volts(&[0.0]),
            offset: Some(volts(&[0.0, g4])),
            scale: Some(spec_signal("C(major)")),
            gate: None,
        });
        assert_eq!(q.channel_count(), 2);

        // The first sample counts as a note change on every channel.
        q.update(48000.0);
        assert_eq!(q.outputs.trig.get(1), GATE_HIGH_VOLTAGE);
        assert!(q.outputs.output.get(0).abs() < 1e-6);
        assert!((q.outputs.output.get(1) - g4).abs() < 1e-4);
    }

    #[test]
    fn test_temp_gate_trigger_behavior() {
        let mut trigger = TempGate::new_gate(TempGateState::Low);
        assert_eq!(trigger.process(), GATE_LOW_VOLTAGE);

        trigger.set_state(TempGateState::High, TempGateState::Low, 3);
        assert_eq!(trigger.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(trigger.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(trigger.process(), GATE_HIGH_VOLTAGE);
        assert_eq!(trigger.process(), GATE_LOW_VOLTAGE);
    }
}
