//! Scales for `$ensemble`: the `scale` param, the Ensemble Oscillator's preset
//! scales, and the lookup that gives a voice the two scale notes around its
//! root and how far it sits between them.
//!
//! Adapted from the 4ms Ensemble Oscillator. Copyright 4ms Company. Used under
//! GPL v3.

use std::borrow::Cow;

use deserr::{DeserializeError, Deserr, ErrorKind, IntoValue, ValuePointerRef};
use schemars::JsonSchema;

use crate::Patch;
use crate::dsp::utilities::quantizer::ScaleSignal;
use crate::poly::PORT_MAX_CHANNELS;
use crate::types::Connect;

/// Notes closer than a tenth of a semitone (in volts) count as one.
const UNICITY_THRESHOLD: f32 = 0.1 / 12.0;

/// The Ensemble Oscillator's Octave and Free scale banks. Each lists its notes
/// in semitones from the first, and the interval at which the scale repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, JsonSchema)]
pub enum Preset {
    /// Harmonics 2–3: root and perfect fifth.
    #[serde(rename = "enosc.harm2-3")]
    Harm2To3,
    /// Harmonics 4–7.
    #[serde(rename = "enosc.harm4-7")]
    Harm4To7,
    /// Harmonics 8–15.
    #[serde(rename = "enosc.harm8-15")]
    Harm8To15,
    /// The odd harmonics up to 15, repeating every three octaves.
    #[serde(rename = "enosc.harm-odd")]
    HarmOdd,
    /// Pythagorean do, fa, so.
    #[serde(rename = "enosc.pythag3")]
    Pythag3,
    /// Pythagorean do, re, fa, so, la.
    #[serde(rename = "enosc.pythag5")]
    Pythag5,
    /// Just 1, 4/3, 3/2, 5/3.
    #[serde(rename = "enosc.just4")]
    Just4,
    /// Just 1, 5/4, 4/3, 3/2, 5/3, 7/4.
    #[serde(rename = "enosc.just6")]
    Just6,
    /// 5 equal steps per octave.
    #[serde(rename = "enosc.5tet")]
    Tet5,
    /// 7 equal steps per octave.
    #[serde(rename = "enosc.7tet")]
    Tet7,
    /// A spiral of just perfect fifths.
    #[serde(rename = "enosc.fifths")]
    Fifths,
    /// Messiaen's first mode of limited transposition: the whole-tone scale.
    #[serde(rename = "enosc.mlt1")]
    Mlt1,
    /// Messiaen's second mode: half step, whole step.
    #[serde(rename = "enosc.mlt2")]
    Mlt2,
    /// Messiaen's third mode.
    #[serde(rename = "enosc.mlt3")]
    Mlt3,
    /// Messiaen's fourth mode.
    #[serde(rename = "enosc.mlt4")]
    Mlt4,
    /// The Bohlen–Pierce scale: 13 equal steps per tritave (3:1).
    #[serde(rename = "enosc.bp")]
    BohlenPierce,
    /// The Bohlen–Pierce major triad, repeating every tritave.
    #[serde(rename = "enosc.bp-major")]
    BohlenPierceMajor,
    /// The Bohlen–Pierce minor triad, repeating every tritave.
    #[serde(rename = "enosc.bp-minor")]
    BohlenPierceMinor,
    /// Wendy Carlos's alpha scale: equal steps of 78 cents.
    #[serde(rename = "enosc.alpha")]
    CarlosAlpha,
    /// Wendy Carlos's beta scale: equal steps of 63.8 cents.
    #[serde(rename = "enosc.beta")]
    CarlosBeta,
}

const PRESETS: &[(&str, Preset)] = &[
    ("enosc.harm2-3", Preset::Harm2To3),
    ("enosc.harm4-7", Preset::Harm4To7),
    ("enosc.harm8-15", Preset::Harm8To15),
    ("enosc.harm-odd", Preset::HarmOdd),
    ("enosc.pythag3", Preset::Pythag3),
    ("enosc.pythag5", Preset::Pythag5),
    ("enosc.just4", Preset::Just4),
    ("enosc.just6", Preset::Just6),
    ("enosc.5tet", Preset::Tet5),
    ("enosc.7tet", Preset::Tet7),
    ("enosc.fifths", Preset::Fifths),
    ("enosc.mlt1", Preset::Mlt1),
    ("enosc.mlt2", Preset::Mlt2),
    ("enosc.mlt3", Preset::Mlt3),
    ("enosc.mlt4", Preset::Mlt4),
    ("enosc.bp", Preset::BohlenPierce),
    ("enosc.bp-major", Preset::BohlenPierceMajor),
    ("enosc.bp-minor", Preset::BohlenPierceMinor),
    ("enosc.alpha", Preset::CarlosAlpha),
    ("enosc.beta", Preset::CarlosBeta),
];

/// A just perfect fifth, in semitones.
const FIFTH: f32 = 7.019_55;
/// A Bohlen–Pierce step and tritave, in semitones.
const BP_STEP: f32 = 1.463_042_3;
const TRITAVE: f32 = 19.019_55;

impl Preset {
    /// Notes in semitones from the first, and the repeat interval.
    fn notes(self) -> (&'static [f32], f32) {
        const BP: [f32; 13] = {
            let mut steps = [0.0; 13];
            let mut i = 0;
            while i < 13 {
                steps[i] = i as f32 * BP_STEP;
                i += 1;
            }
            steps
        };
        match self {
            Preset::Harm2To3 => (&[0.0, FIFTH], 12.0),
            Preset::Harm4To7 => (&[0.0, 3.863_137, FIFTH, 9.688_259], 12.0),
            Preset::Harm8To15 => (
                &[
                    0.0, 2.039_1, 3.863_137, 5.513_179, FIFTH, 8.405_277, 9.688_259, 10.882_687,
                ],
                12.0,
            ),
            Preset::HarmOdd => (
                &[
                    0.0, FIFTH, 15.863_137, 21.688_26, 26.039_1, 29.513_18, 32.405_277, 34.882_687,
                ],
                36.0,
            ),
            Preset::Pythag3 => (&[0.0, 4.980_45, FIFTH], 12.0),
            Preset::Pythag5 => (&[0.0, 2.039_1, 4.980_45, FIFTH, 9.058_65], 12.0),
            Preset::Just4 => (&[0.0, 4.980_45, FIFTH, 8.843_587], 12.0),
            Preset::Just6 => (
                &[0.0, 3.863_137, 4.980_45, FIFTH, 8.843_587, 9.688_259],
                12.0,
            ),
            Preset::Tet5 => (&[0.0, 2.4, 4.8, 7.2, 9.6], 12.0),
            Preset::Tet7 => (
                &[
                    0.0,
                    1.714_285_7,
                    3.428_571_4,
                    5.142_857,
                    6.857_143,
                    8.571_428,
                    10.285_714,
                ],
                12.0,
            ),
            Preset::Fifths => (&[0.0], FIFTH),
            Preset::Mlt1 => (&[0.0], 2.0),
            Preset::Mlt2 => (&[0.0, 1.0], 3.0),
            Preset::Mlt3 => (&[0.0, 2.0, 3.0], 4.0),
            Preset::Mlt4 => (&[0.0, 1.0, 2.0, 5.0], 6.0),
            Preset::BohlenPierce => (&BP, TRITAVE),
            Preset::BohlenPierceMajor => (&[0.0, 8.778_254, 14.630_423], TRITAVE),
            Preset::BohlenPierceMinor => (&[0.0, 5.852_169, 14.630_423], TRITAVE),
            Preset::CarlosAlpha => (&[0.0], 0.78),
            Preset::CarlosBeta => (&[0.0], 0.638),
        }
    }
}

/// The `scale` param: an Ensemble Oscillator preset name, or notes as for
/// `$quantizer` (a poly V/Oct signal or a spec string, which the DSL turns into
/// a `$chord`).
#[derive(Clone)]
pub(super) enum EnsembleScale {
    Preset(Preset),
    Notes(ScaleSignal),
}

impl Connect for EnsembleScale {
    fn apply_default_connections(&mut self) {
        if let EnsembleScale::Notes(notes) = self {
            notes.apply_default_connections();
        }
    }
    fn connect(&mut self, patch: &Patch) {
        if let EnsembleScale::Notes(notes) = self {
            notes.connect(patch);
        }
    }
    fn collect_cables(&self, sink: &mut Vec<String>) {
        if let EnsembleScale::Notes(notes) = self {
            notes.collect_cables(sink);
        }
    }
    fn inject_index_ptr(&mut self, ptr: *const std::cell::Cell<usize>) {
        if let EnsembleScale::Notes(notes) = self {
            notes.inject_index_ptr(ptr);
        }
    }
    fn mark_glides(
        &mut self,
        old: Option<&Self>,
        generation: u64,
        ids: &dyn crate::types::PreviousIds,
    ) {
        if let EnsembleScale::Notes(notes) = self {
            let old = match old {
                Some(EnsembleScale::Notes(old)) => Some(old),
                _ => None,
            };
            notes.mark_glides(old, generation, ids);
        }
    }
}

impl JsonSchema for EnsembleScale {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EnsembleScale")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        #[derive(JsonSchema)]
        #[serde(untagged)]
        #[allow(dead_code)]
        enum EnsembleScaleSchema {
            Preset(Preset),
            Notes(ScaleSignal),
        }
        EnsembleScaleSchema::json_schema(generator)
    }
}

impl<E: DeserializeError> Deserr<E> for EnsembleScale {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        match value {
            deserr::Value::String(name) => PRESETS
                .iter()
                .find(|(candidate, _)| *candidate == name)
                .map(|&(_, preset)| EnsembleScale::Preset(preset))
                .ok_or_else(|| {
                    let names: Vec<&str> = PRESETS.iter().map(|(n, _)| *n).collect();
                    deserr::take_cf_content(E::error::<V>(
                        None,
                        ErrorKind::Unexpected {
                            msg: format!(
                                "unknown scale preset \"{name}\"; expected one of {}",
                                names.join(", ")
                            ),
                        },
                        location,
                    ))
                }),
            other => ScaleSignal::deserialize_from_value(other, location).map(EnsembleScale::Notes),
        }
    }
}

/// Two adjacent scale notes in V/Oct and the crossfade between them: 0 sounds
/// `p1`, 1 sounds `p2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PitchPair {
    pub p1: f32,
    pub p2: f32,
    pub crossfade: f32,
}

/// Where a [`ScaleNotes`] came from, to tell when it needs rebuilding.
#[derive(Clone, Copy)]
enum Source {
    Semitones,
    Preset(Preset),
    Signal([f32; PORT_MAX_CHANNELS], usize),
}

/// The scale as sorted, deduplicated intervals in V above its first note, in
/// `[0, period)`, followed by the first note one period up.
#[derive(Clone, Copy)]
pub(super) struct ScaleNotes {
    notes: [f32; PORT_MAX_CHANNELS + 1],
    /// Notes per period.
    len: usize,
    /// Interval in V at which the scale repeats.
    period: f32,
    source: Source,
    /// Bumped on every rebuild, so a cached [`PitchPair`] can tell it is stale.
    pub generation: u64,
}

impl Default for ScaleNotes {
    fn default() -> Self {
        let mut notes = [0.0; PORT_MAX_CHANNELS + 1];
        for (i, note) in notes.iter_mut().take(13).enumerate() {
            *note = i as f32 / 12.0;
        }
        Self {
            notes,
            len: 12,
            period: 1.0,
            source: Source::Semitones,
            generation: 0,
        }
    }
}

impl ScaleNotes {
    /// Rebuild from `scale` if it changed; `None` selects semitones.
    /// Allocation-free.
    pub fn update(&mut self, scale: Option<&EnsembleScale>) {
        let unchanged = match (scale, &self.source) {
            (None, Source::Semitones) => true,
            (Some(EnsembleScale::Preset(preset)), Source::Preset(current)) => preset == current,
            (Some(EnsembleScale::Notes(notes)), Source::Signal(values, n)) => {
                let signal = notes.signal();
                signal.channels().min(PORT_MAX_CHANNELS) == *n
                    && values[..*n]
                        .iter()
                        .enumerate()
                        .all(|(i, &v)| signal.get_value(i) == v)
            }
            _ => false,
        };
        if unchanged {
            return;
        }
        let source = match scale {
            None => Source::Semitones,
            Some(EnsembleScale::Preset(preset)) => Source::Preset(*preset),
            Some(EnsembleScale::Notes(notes)) => {
                let signal = notes.signal();
                let n = signal.channels().min(PORT_MAX_CHANNELS);
                let mut values = [0.0; PORT_MAX_CHANNELS];
                for (i, v) in values.iter_mut().take(n).enumerate() {
                    *v = signal.get_value(i);
                }
                Source::Signal(values, n)
            }
        };
        let generation = self.generation + 1;
        match source {
            Source::Semitones => *self = Self::default(),
            Source::Preset(preset) => {
                let (notes, period) = preset.notes();
                self.period = period / 12.0;
                self.len = notes.len();
                for (dst, &semis) in self.notes.iter_mut().zip(notes) {
                    *dst = semis / 12.0;
                }
                self.notes[self.len] = self.period;
            }
            Source::Signal(values, n) => self.build_octave(&values[..n]),
        }
        self.source = source;
        self.generation = generation;
    }

    /// Octave-repeating intervals above the first finite note of `values`.
    fn build_octave(&mut self, values: &[f32]) {
        let mut finite = values.iter().copied().filter(|v| v.is_finite());
        let Some(tonic) = finite.next() else {
            *self = Self::default();
            return;
        };
        let mut classes = [0.0f32; PORT_MAX_CHANNELS];
        classes[0] = 0.0;
        let mut count = 1;
        for v in finite {
            let class = (v - tonic).rem_euclid(1.0);
            classes[count] = if class < 1.0 { class } else { 0.0 };
            count += 1;
        }
        classes[..count].sort_unstable_by(f32::total_cmp);

        let mut len = 0;
        for &class in &classes[..count] {
            if len == 0 || class - self.notes[len - 1] >= UNICITY_THRESHOLD {
                self.notes[len] = class;
                len += 1;
            }
        }
        if len > 1 && 1.0 - self.notes[len - 1] < UNICITY_THRESHOLD {
            len -= 1;
        }
        self.notes[len] = 1.0;
        self.len = len;
        self.period = 1.0;
    }

    /// The scale notes on either side of `root`, in V above the scale's first
    /// note.
    ///
    /// Consecutive intervals swap which of `p1`/`p2` is the upper note, so as
    /// `root` rises the oscillator fading in on one interval keeps sounding as
    /// the one fading out on the next.
    pub fn pitch_pair(&self, root: f32) -> PitchPair {
        let period = (root / self.period).floor();
        let offset = period * self.period;
        let target = root - offset;
        // The last note at or below `target`; note 0 always is.
        let j = self.notes[1..self.len].partition_point(|&n| n <= target);
        let (lo, hi) = (self.notes[j], self.notes[j + 1]);
        let crossfade = ((target - lo) / (hi - lo)).clamp(0.0, 1.0);
        let (p1, p2) = (lo + offset, hi + offset);

        let interval = j as i64 + period as i64 * self.len as i64;
        if interval.rem_euclid(2) == 1 {
            PitchPair {
                p1: p2,
                p2: p1,
                crossfade: 1.0 - crossfade,
            }
        } else {
            PitchPair { p1, p2, crossfade }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::Signal;

    fn scale(volts: &[f32]) -> ScaleNotes {
        let signals: Vec<Signal> = volts.iter().map(|&v| Signal::Volts(v)).collect();
        let value = serde_json::to_value(PolySignal::poly(&signals)).unwrap();
        let scale =
            deserr::deserialize::<EnsembleScale, _, deserr::errors::JsonError>(value).unwrap();
        let mut notes = ScaleNotes::default();
        notes.update(Some(&scale));
        notes
    }

    fn preset(name: &str) -> ScaleNotes {
        let scale = deserr::deserialize::<EnsembleScale, _, deserr::errors::JsonError>(
            serde_json::Value::String(name.to_string()),
        )
        .unwrap();
        let mut notes = ScaleNotes::default();
        notes.update(Some(&scale));
        notes
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn sounding(pair: PitchPair) -> f32 {
        if pair.crossfade < 0.5 {
            pair.p1
        } else {
            pair.p2
        }
    }

    #[test]
    fn on_a_note_sounds_that_note_alone() {
        let major_triad = scale(&[0.0, 4.0 / 12.0, 7.0 / 12.0]);
        let pair = major_triad.pitch_pair(4.0 / 12.0);
        assert!(close(sounding(pair), 4.0 / 12.0), "{pair:?}");
        assert!(pair.crossfade == 0.0 || pair.crossfade == 1.0, "{pair:?}");
    }

    #[test]
    fn crossfade_tracks_position_between_notes() {
        let fifths = scale(&[0.0, 7.0 / 12.0]);
        let pair = fifths.pitch_pair(3.5 / 12.0);
        let (lo, hi, t) = if pair.p1 < pair.p2 {
            (pair.p1, pair.p2, pair.crossfade)
        } else {
            (pair.p2, pair.p1, 1.0 - pair.crossfade)
        };
        assert!(close(lo, 0.0) && close(hi, 7.0 / 12.0) && close(t, 0.5));
    }

    #[test]
    fn the_sounding_oscillator_carries_across_intervals() {
        // Rising through a note, the oscillator that faded in keeps the note:
        // the slot sounding just below note N is the slot sounding just above.
        let slot = |p: PitchPair| if p.crossfade > 0.5 { 1 } else { 0 };
        for (notes, at) in [
            (
                scale(&[0.0, 3.0 / 12.0, 7.0 / 12.0]),
                vec![3.0 / 12.0, 7.0 / 12.0, 1.0, -5.0 / 12.0],
            ),
            (
                preset("enosc.bp-minor"),
                vec![5.852_169 / 12.0, TRITAVE / 12.0],
            ),
            (
                preset("enosc.fifths"),
                vec![FIFTH / 12.0, 2.0 * FIFTH / 12.0],
            ),
        ] {
            for note in at {
                let below = notes.pitch_pair(note - 1e-4);
                let above = notes.pitch_pair(note + 1e-4);
                assert_eq!(slot(below), slot(above), "at {note}: {below:?} {above:?}");
            }
        }
    }

    #[test]
    fn notes_are_intervals_above_the_first() {
        // A minor's notes, voiced across octaves: the scale starts on A.
        let a = 9.0 / 12.0;
        let notes = scale(&[a, a + 2.0 / 12.0 - 1.0, a + 3.0 / 12.0 + 2.0]);
        assert_eq!(notes.len, 3);
        assert!(close(notes.notes[1], 2.0 / 12.0) && close(notes.notes[2], 3.0 / 12.0));
        assert!(close(notes.notes[3], 1.0));
    }

    #[test]
    fn duplicate_notes_merge() {
        let notes = scale(&[0.0, 7.0 / 12.0, 2.0 + 7.0 / 12.0, -1.0]);
        assert_eq!(notes.len, 2);
    }

    #[test]
    fn default_is_semitones() {
        let pair = ScaleNotes::default().pitch_pair(5.25 / 12.0);
        assert!(close(pair.p1.min(pair.p2), 5.0 / 12.0), "{pair:?}");
    }

    #[test]
    fn free_presets_repeat_at_their_period() {
        // Bohlen–Pierce repeats at the tritave, not the octave.
        let bp = preset("enosc.bp-major");
        let pair = bp.pitch_pair(TRITAVE / 12.0 + 8.778_254 / 12.0);
        assert!(
            close(sounding(pair), (TRITAVE + 8.778_254) / 12.0),
            "{pair:?}"
        );
        // A fifths spiral steps a fifth at a time.
        let fifths = preset("enosc.fifths");
        let pair = fifths.pitch_pair(3.0 * FIFTH / 12.0);
        assert!(close(sounding(pair), 3.0 * FIFTH / 12.0), "{pair:?}");
    }

    #[test]
    fn every_preset_is_named_and_ascending() {
        let schema = serde_json::to_value(schemars::schema_for!(Preset))
            .unwrap()
            .to_string();
        for &(name, preset) in PRESETS {
            assert!(
                schema.contains(&format!("\"{name}\"")),
                "{name} missing from schema"
            );
            let (notes, period) = preset.notes();
            assert!(notes[0] == 0.0, "{name}");
            assert!(notes.windows(2).all(|w| w[0] < w[1]), "{name}");
            assert!(*notes.last().unwrap() < period, "{name}");
        }
    }

    #[test]
    fn unknown_preset_is_rejected() {
        assert!(
            deserr::deserialize::<EnsembleScale, _, deserr::errors::JsonError>(
                serde_json::Value::String("enosc.nope".to_string())
            )
            .is_err()
        );
    }
}
