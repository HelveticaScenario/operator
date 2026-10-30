//! Scales for `$enosc`: the Ensemble Oscillator's 12-TET and Octave banks,
//! custom scales, and the lookup that gives a voice the two scale notes around
//! its root and how far it sits between them.
//!
//! Adapted from the 4ms Ensemble Oscillator. Copyright 4ms Company. Used under
//! GPL v3.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use deserr::{DeserializeError, Deserr, ErrorKind, IntoValue, Map as _, ValuePointerRef};
use schemars::JsonSchema;

use crate::Patch;
use crate::types::{Connect, PreviousIds};

/// Most notes in a scale, counting the note that closes its period.
const MAX_NOTES: usize = 32;

/// Notes closer than a tenth of a semitone count as one.
const UNICITY_THRESHOLD: f32 = 0.1;

/// Highest note of a custom scale, in semitones above its lowest.
const MAX_SPAN: f32 = 150.0;

/// The Ensemble Oscillator's 12-TET and Octave scales, in semitones. The last
/// note of each is its period: the interval at which it repeats.
const PRESETS: &[(&str, &[f32])] = &[
    // 12-TET
    ("octave", &[0.0, 12.0]),
    ("octaveFifth", &[0.0, 7.0, 12.0]),
    ("major", &[0.0, 4.0, 7.0, 12.0]),
    ("minor", &[0.0, 3.0, 7.0, 12.0]),
    ("minorAdd4", &[0.0, 3.0, 5.0, 7.0, 12.0]),
    ("quartal", &[0.0, 3.0, 5.0, 10.0, 12.0]),
    ("miyako", &[0.0, 1.0, 5.0, 8.0, 12.0]),
    ("fifths", &[0.0, 7.0]),
    ("fifthsMinor", &[0.0, 3.0, 5.0, 7.0]),
    ("chromatic", &[0.0, 1.0]),
    // Octave
    ("harm2to3", &[0.0, 7.019_55, 12.0]),
    ("harm4to7", &[0.0, 3.863_137, 7.019_55, 9.688_259, 12.0]),
    (
        "harm8to15",
        &[
            0.0, 2.039_1, 3.863_137, 5.513_179, 7.019_55, 8.405_277, 9.688_259, 10.882_687, 12.0,
        ],
    ),
    (
        "harmOdd",
        &[
            0.0, 7.019_55, 15.863_137, 21.688_26, 26.039_1, 29.513_18, 32.405_277, 34.882_687, 36.0,
        ],
    ),
    ("pythag3", &[0.0, 4.980_45, 7.019_55, 12.0]),
    (
        "pythag5",
        &[0.0, 2.039_1, 4.980_45, 7.019_55, 9.058_65, 12.0],
    ),
    ("just4", &[0.0, 4.980_45, 7.019_55, 8.843_587, 12.0]),
    (
        "just6",
        &[
            0.0, 3.863_137, 4.980_45, 7.019_55, 8.843_587, 9.688_259, 12.0,
        ],
    ),
    ("tet5", &[0.0, 2.4, 4.8, 7.2, 9.6, 12.0]),
    (
        "tet7",
        &[
            0.0,
            1.714_285_7,
            3.428_571_4,
            5.142_857,
            6.857_143,
            8.571_428,
            10.285_714,
            12.0,
        ],
    ),
];

/// Hands every deserialized scale its own [`ScaleNotes::generation`], so a
/// voice's cached lookup goes stale when the scale is replaced.
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// The `scale` param: a preset (`{ "type": "major" }`) or a custom scale
/// (`{ "type": "custom", "notes": [0, 3, 7], "repeatsOctave": true }`).
#[derive(Clone)]
pub(super) struct EnoscScale(pub ScaleNotes);

impl Connect for EnoscScale {
    fn apply_default_connections(&mut self) {}
    fn connect(&mut self, _patch: &Patch) {}
    fn collect_cables(&self, _sink: &mut Vec<String>) {}
    fn inject_index_ptr(&mut self, _ptr: *const std::cell::Cell<usize>) {}
    fn mark_glides(&mut self, _old: Option<&Self>, _generation: u64, _ids: &dyn PreviousIds) {}
}

impl JsonSchema for EnoscScale {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EnoscScale")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        #[derive(JsonSchema)]
        #[serde(tag = "type", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum EnoscScaleSchema {
            Octave,
            OctaveFifth,
            Major,
            Minor,
            MinorAdd4,
            Quartal,
            Miyako,
            Fifths,
            FifthsMinor,
            Chromatic,
            Harm2to3,
            Harm4to7,
            Harm8to15,
            HarmOdd,
            Pythag3,
            Pythag5,
            Just4,
            Just6,
            Tet5,
            Tet7,
            #[serde(rename_all = "camelCase")]
            Custom {
                notes: Vec<f32>,
                repeats_octave: bool,
            },
        }
        EnoscScaleSchema::json_schema(generator)
    }
}

impl<E: DeserializeError> Deserr<E> for EnoscScale {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        let error =
            |kind: ErrorKind<V>| deserr::take_cf_content(E::error::<V>(None, kind, location));
        let unexpected = |msg: String| error(ErrorKind::Unexpected { msg });

        let deserr::Value::Map(mut map) = value else {
            return Err(unexpected(
                "a scale must be an object with a \"type\" field; build one with $enosc.scale.*"
                    .to_string(),
            ));
        };
        let kind = match map.remove("type").map(IntoValue::into_value) {
            Some(deserr::Value::String(kind)) => kind,
            _ => return Err(error(ErrorKind::MissingField { field: "type" })),
        };

        if kind == "custom" {
            let notes = map
                .remove("notes")
                .ok_or_else(|| error(ErrorKind::MissingField { field: "notes" }))?;
            let notes = Vec::<f32>::deserialize_from_value(notes.into_value(), location)?;
            let repeats_octave = map.remove("repeatsOctave").ok_or_else(|| {
                error(ErrorKind::MissingField {
                    field: "repeatsOctave",
                })
            })?;
            let repeats_octave =
                bool::deserialize_from_value(repeats_octave.into_value(), location)?;
            return ScaleNotes::custom(&notes, repeats_octave)
                .map(EnoscScale)
                .map_err(unexpected);
        }

        PRESETS
            .iter()
            .find(|(name, _)| *name == kind)
            .map(|(_, notes)| EnoscScale(ScaleNotes::from_semitones(notes)))
            .ok_or_else(|| {
                let names: Vec<&str> = PRESETS.iter().map(|(name, _)| *name).collect();
                unexpected(format!(
                    "unknown scale \"{kind}\"; expected custom or one of {}",
                    names.join(", ")
                ))
            })
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

/// The scale as ascending notes in V above its first, starting at 0, with the
/// note that closes the period last.
#[derive(Clone, Copy)]
pub(super) struct ScaleNotes {
    notes: [f32; MAX_NOTES],
    /// Notes per period; `notes[len]` is the period.
    len: usize,
    /// Distinct for every scale, so a cached [`PitchPair`] can tell it is
    /// stale.
    pub generation: u64,
}

impl ScaleNotes {
    /// Every semitone, the scale when none is given.
    pub const CHROMATIC: ScaleNotes = {
        let mut notes = [0.0; MAX_NOTES];
        notes[1] = 1.0 / 12.0;
        ScaleNotes {
            notes,
            len: 1,
            generation: 0,
        }
    };

    /// A scale from ascending semitones that start at 0 and end on the period.
    fn from_semitones(semitones: &[f32]) -> Self {
        let mut notes = [0.0; MAX_NOTES];
        for (dst, &semis) in notes.iter_mut().zip(semitones) {
            *dst = semis / 12.0;
        }
        Self {
            notes,
            len: semitones.len() - 1,
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// A scale from notes in semitones, in any order, measured from the lowest.
    /// With `repeats_octave` the scale repeats at the first octave at or above
    /// its highest note; otherwise the highest note is the period.
    fn custom(notes: &[f32], repeats_octave: bool) -> Result<Self, String> {
        if notes.iter().any(|n| !n.is_finite()) {
            return Err("scale notes must be finite numbers".to_string());
        }
        let mut sorted = notes.to_vec();
        sorted.sort_unstable_by(f32::total_cmp);
        let Some(&lowest) = sorted.first() else {
            return Err("a custom scale needs at least one note".to_string());
        };

        let mut semitones: Vec<f32> = Vec::with_capacity(sorted.len() + 1);
        for note in sorted {
            let note = note - lowest;
            if semitones
                .last()
                .is_none_or(|&last| note - last >= UNICITY_THRESHOLD)
            {
                semitones.push(note);
            }
        }
        if repeats_octave {
            let highest = *semitones.last().unwrap_or(&0.0);
            let octave = ((highest - UNICITY_THRESHOLD) / 12.0).floor().max(0.0) + 1.0;
            let period = octave * 12.0;
            if period - highest < UNICITY_THRESHOLD {
                semitones.pop();
            }
            semitones.push(period);
        }

        if semitones.len() < 2 {
            return Err(
                "a custom scale that does not repeat every octave needs two distinct notes; \
                 the highest is its period"
                    .to_string(),
            );
        }
        if semitones.len() > MAX_NOTES {
            return Err(format!(
                "a custom scale holds at most {} distinct notes",
                MAX_NOTES - usize::from(repeats_octave)
            ));
        }
        if semitones[semitones.len() - 1] > MAX_SPAN {
            return Err(format!("a custom scale spans at most {MAX_SPAN} semitones"));
        }
        Ok(Self::from_semitones(&semitones))
    }

    /// The scale notes on either side of `root`, in V above the scale's first
    /// note.
    ///
    /// Consecutive intervals swap which of `p1`/`p2` is the upper note, so as
    /// `root` rises the oscillator fading in on one interval keeps sounding as
    /// the one fading out on the next.
    pub fn pitch_pair(&self, root: f32) -> PitchPair {
        let period_len = self.notes[self.len];
        let period = (root / period_len).floor();
        let offset = period * period_len;
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
    use serde_json::{Value, json};

    fn parse(value: Value) -> Result<ScaleNotes, deserr::errors::JsonError> {
        deserr::deserialize::<EnoscScale, _, deserr::errors::JsonError>(value).map(|s| s.0)
    }

    fn custom(notes: &[f32], repeats_octave: bool) -> ScaleNotes {
        parse(json!({ "type": "custom", "notes": notes, "repeatsOctave": repeats_octave })).unwrap()
    }

    fn preset(name: &str) -> ScaleNotes {
        parse(json!({ "type": name })).unwrap()
    }

    fn semitones(scale: &ScaleNotes) -> Vec<f32> {
        scale.notes[..=scale.len]
            .iter()
            .map(|v| (v * 12.0 * 1e4).round() / 1e4)
            .collect()
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
        let pair = preset("major").pitch_pair(4.0 / 12.0);
        assert!(close(sounding(pair), 4.0 / 12.0), "{pair:?}");
        assert!(pair.crossfade == 0.0 || pair.crossfade == 1.0, "{pair:?}");
    }

    #[test]
    fn crossfade_tracks_position_between_notes() {
        let pair = preset("fifths").pitch_pair(3.5 / 12.0);
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
                preset("minor"),
                vec![3.0 / 12.0, 7.0 / 12.0, 1.0, -5.0 / 12.0],
            ),
            (preset("harmOdd"), vec![7.019_55 / 12.0, 3.0]),
            (preset("fifths"), vec![7.0 / 12.0, 14.0 / 12.0]),
            (ScaleNotes::CHROMATIC, vec![1.0 / 12.0, 2.0 / 12.0]),
        ] {
            for note in at {
                let below = notes.pitch_pair(note - 1e-4);
                let above = notes.pitch_pair(note + 1e-4);
                assert_eq!(slot(below), slot(above), "at {note}: {below:?} {above:?}");
            }
        }
    }

    #[test]
    fn presets_repeat_at_their_last_note() {
        // `fifthsMinor` repeats every fifth: 3 semitones above 7 is 10.
        let pair = preset("fifthsMinor").pitch_pair(10.0 / 12.0);
        assert!(close(sounding(pair), 10.0 / 12.0), "{pair:?}");
        // The odd harmonics repeat every three octaves.
        let pair = preset("harmOdd").pitch_pair(3.0 + 15.863_137 / 12.0);
        assert!(close(sounding(pair), 3.0 + 15.863_137 / 12.0), "{pair:?}");
    }

    #[test]
    fn custom_notes_measure_from_the_lowest() {
        assert_eq!(
            semitones(&custom(&[7.0, 3.0, 10.0], false)),
            [0.0, 4.0, 7.0]
        );
    }

    #[test]
    fn custom_octave_repeat_closes_at_the_next_octave() {
        assert_eq!(
            semitones(&custom(&[0.0, 4.0, 7.0], true)),
            [0.0, 4.0, 7.0, 12.0]
        );
        // A note on the octave is the period itself.
        assert_eq!(
            semitones(&custom(&[0.0, 4.0, 7.0, 12.0], true)),
            [0.0, 4.0, 7.0, 12.0]
        );
        assert_eq!(semitones(&custom(&[0.0, 14.0], true)), [0.0, 14.0, 24.0]);
        assert_eq!(semitones(&custom(&[5.0], true)), [0.0, 12.0]);
    }

    #[test]
    fn custom_merges_near_duplicates() {
        assert_eq!(
            semitones(&custom(&[0.0, 0.05, 7.0, 7.0], false)),
            [0.0, 7.0]
        );
    }

    #[test]
    fn invalid_scales_are_rejected() {
        for value in [
            json!({ "type": "nope" }),
            json!("major"),
            json!({ "type": "custom", "notes": [0], "repeatsOctave": false }),
            json!({ "type": "custom", "notes": [], "repeatsOctave": true }),
            json!({ "type": "custom", "notes": [0, 200], "repeatsOctave": false }),
            json!({ "type": "custom", "notes": [0, 7] }),
            json!({ "type": "custom", "notes": (0..40).collect::<Vec<_>>(), "repeatsOctave": false }),
        ] {
            assert!(parse(value.clone()).is_err(), "{value}");
        }
    }

    #[test]
    fn every_preset_is_in_the_schema_and_ascending() {
        let schema = serde_json::to_value(schemars::schema_for!(EnoscScale))
            .unwrap()
            .to_string();
        for &(name, notes) in PRESETS {
            assert!(
                schema.contains(&format!("\"{name}\"")),
                "{name} missing from schema"
            );
            assert!(notes[0] == 0.0, "{name}");
            assert!(notes.windows(2).all(|w| w[0] < w[1]), "{name}");
        }
    }

    #[test]
    fn every_scale_is_its_own_generation() {
        assert_ne!(preset("major").generation, preset("major").generation);
        assert_ne!(preset("major").generation, ScaleNotes::CHROMATIC.generation);
    }
}
