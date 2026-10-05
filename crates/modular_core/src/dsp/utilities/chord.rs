use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::PolyOutput;

use super::scale::ScaleSpec;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ChordParams {
    /// scale or chord spec, e.g. "C(major)", "Eb3[m7]", "C[just maj7 inv1]"
    chord: ScaleSpec,
}

/// One channel per note of the spec.
#[allow(private_interfaces)]
pub fn chord_derive_channel_count(params: &ChordParams) -> usize {
    params.chord.voltages().len()
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ChordOutputs {
    #[output("output", "V/Oct pitch of each note, ascending", default)]
    output: PolyOutput,
}

/// Outputs the notes of a scale or chord as a polyphonic V/Oct signal, one
/// channel per note in ascending pitch.
///
/// The spec is a root note (optional octave, default 4) followed by a scale in
/// `()` or a chord in `[]`:
///
/// | Form | Example | Notes |
/// |------|---------|-------|
/// | `root(scale)` | `"C(major)"`, `"D3(dorian)"` | Named scale, one octave |
/// | `root(intervals)` | `"C(0 3 7 10)"` | Semitones from the root, reduced to one octave |
/// | `root(tuning)` | `"C(just)"`, `"A(pythag)"` | All 12 steps in a non-equal tuning |
/// | `root[chord]` | `"C[maj7]"`, `"F#2[m9]"` | Chord shorthand; extensions keep their register |
/// | `root[intervals]` | `"C[0 7 16]"` | Custom voicing; intervals keep their register |
/// | `root[tuning chord]` | `"C[just maj]"` | Tuning prefix (`just`, `pythag`, `pythagorean`) |
/// | `root[chord invN]` | `"C[maj7 inv1]"` | Raises the lowest N notes an octave |
/// | `chromatic` | `"chromatic"` | All 12 semitones from C4 |
///
/// Scale names: major/maj/ionian/M, minor/min/aeolian/m, dorian, phrygian,
/// lydian, mixolydian, locrian, harmonic minor, melodic minor, pentatonic
/// major/minor, blues, whole tone, chromatic.
///
/// Chord names (case-sensitive): maj/M/major, min/m/minor, dim/°, aug/+,
/// sus2, sus4/sus, 5, 6, m6, 69, 7/dom7, maj7/M7/Δ/Δ7, m7/min7, mMaj7/mM7,
/// dim7/°7, m7b5/ø/ø7, aug7/7#5/+7, maj7#5, 7sus4, 7sus2, 7b5, 9, maj9, m9,
/// 11, maj11, m11, 13, maj13, m13, 7b9, 7#9, 7#11, add9/add2, madd9,
/// add11/add4.
///
/// ```js
/// // a C major 7th chord on four saw voices
/// $saw($chord("C3[maj7]")).out()
/// ```
///
/// ```js
/// // quantize a slow LFO to the notes of a minor 9th chord
/// $sine($quantizer($sine("0.1hz").range(-1, 1), $chord("A[m9]"))).out()
/// ```
#[module(name = "$chord", channels_derive = chord_derive_channel_count, args(chord))]
pub struct Chord {
    outputs: ChordOutputs,
    params: ChordParams,
}

impl Chord {
    fn update(&mut self, _sample_rate: f32) {
        for (ch, &v) in self.params.chord.voltages().iter().enumerate() {
            self.outputs.output.set(ch, v);
        }
    }
}

message_handlers!(impl Chord {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OutputStruct;

    fn make_chord(spec: &str) -> Chord {
        let params = ChordParams {
            chord: ScaleSpec::parse(spec).unwrap(),
        };
        let channels = chord_derive_channel_count(&params);
        let mut outputs = ChordOutputs::default();
        outputs.set_all_channels(channels);
        Chord {
            params,
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
        }
    }

    #[test]
    fn outputs_one_channel_per_note() {
        let mut chord = make_chord("C3[maj7]");
        assert_eq!(chord.channel_count(), 4);
        chord.update(48000.0);
        let expected = [-12.0, -8.0, -5.0, -1.0];
        for (ch, semis) in expected.iter().enumerate() {
            let v = chord.outputs.output.get(ch);
            assert!((v - semis / 12.0).abs() < 1e-6, "ch {ch}: {v}");
        }
    }

    #[test]
    fn scales_output_every_degree() {
        assert_eq!(make_chord("C(major)").channel_count(), 7);
        assert_eq!(make_chord("chromatic").channel_count(), 12);
    }

    #[test]
    fn just_tuning_applies_to_chord_notes() {
        let mut chord = make_chord("C[just maj]");
        chord.update(48000.0);
        assert!((chord.outputs.output.get(1) as f64 - 1.25_f64.log2()).abs() < 1e-6);
    }
}
