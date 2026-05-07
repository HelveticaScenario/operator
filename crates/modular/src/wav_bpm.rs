use std::path::Path;

use aubio::{OnsetMode, Tempo};
use regex::Regex;

const BPM_MIN: u32 = 40;
const BPM_MAX: u32 = 300;
const ANALYSIS_MIN_SECONDS: f64 = 2.0;
const ANALYSIS_MIN_CONFIDENCE: f32 = 0.1;
const TEMPO_BUF_SIZE: usize = 1024;
const TEMPO_HOP_SIZE: usize = 512;

/// Parse a BPM hint from the file stem.
///
/// Two patterns, in priority order:
///   1. `(?i)(\d{2,3})\s*bpm` — explicit, e.g. `loop_140bpm`.
///   2. `(\d{2,3})$` — trailing 2–3 digit number on the stem, e.g. `breaks125`.
///
/// Only values in [BPM_MIN, BPM_MAX] are accepted.
pub fn parse_bpm_from_filename(path: &Path) -> Option<f64> {
  let stem = path.file_stem()?.to_str()?;

  let explicit_re = Regex::new(r"(?i)(\d{2,3})\s*bpm").ok()?;
  if let Some(caps) = explicit_re.captures(stem)
    && let Some(m) = caps.get(1)
    && let Ok(n) = m.as_str().parse::<u32>()
    && (BPM_MIN..=BPM_MAX).contains(&n)
  {
    return Some(n as f64);
  }

  let trailing_re = Regex::new(r"(\d{2,3})$").ok()?;
  if let Some(caps) = trailing_re.captures(stem)
    && let Some(m) = caps.get(1)
    && let Ok(n) = m.as_str().parse::<u32>()
    && (BPM_MIN..=BPM_MAX).contains(&n)
  {
    return Some(n as f64);
  }

  None
}

/// Estimate BPM by feeding mono samples through aubio's `Tempo` detector.
///
/// Returns `None` when:
/// - the buffer is shorter than `ANALYSIS_MIN_SECONDS`,
/// - aubio fails to construct (rare — only on degenerate sample rates),
/// - confidence falls below `ANALYSIS_MIN_CONFIDENCE`,
/// - the resulting BPM is outside `[BPM_MIN, BPM_MAX]`.
pub fn estimate_bpm_from_audio(samples: &[f32], sample_rate: u32) -> Option<f64> {
  if sample_rate == 0 {
    return None;
  }
  if (samples.len() as f64) / (sample_rate as f64) < ANALYSIS_MIN_SECONDS {
    return None;
  }

  let mut tempo = Tempo::new(
    OnsetMode::SpecFlux,
    TEMPO_BUF_SIZE,
    TEMPO_HOP_SIZE,
    sample_rate,
  )
  .ok()?;

  for chunk in samples.chunks_exact(TEMPO_HOP_SIZE) {
    tempo.do_result(chunk).ok()?;
  }

  let confidence = tempo.get_confidence();
  if confidence < ANALYSIS_MIN_CONFIDENCE {
    return None;
  }

  let bpm = tempo.get_bpm();
  if !bpm.is_finite() || bpm < BPM_MIN as f32 || bpm > BPM_MAX as f32 {
    return None;
  }

  Some(bpm as f64)
}

/// Number of bars the sample spans, given BPM and time signature.
///
/// `ts.0` = numerator (beats per bar), `ts.1` = denominator (note value of one beat).
/// BPM is conventionally expressed in quarter notes, so the conversion factor is
/// `4 / denominator`. Default time signature is `(4, 4)`.
pub fn compute_bar_count(duration_seconds: f64, bpm: f64, ts: (u16, u16)) -> f64 {
  let bar_seconds = (60.0 * ts.0 as f64 * 4.0) / (bpm * ts.1 as f64);
  duration_seconds / bar_seconds
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
  }

  #[test]
  fn filename_trailing_digits() {
    assert_eq!(parse_bpm_from_filename(&p("breaks125.wav")), Some(125.0));
    assert_eq!(parse_bpm_from_filename(&p("cw_amen01_175.wav")), Some(175.0));
  }

  #[test]
  fn filename_explicit_bpm_suffix() {
    assert_eq!(parse_bpm_from_filename(&p("loop_140bpm.wav")), Some(140.0));
    assert_eq!(parse_bpm_from_filename(&p("LOOP_90BPM.wav")), Some(90.0));
    assert_eq!(parse_bpm_from_filename(&p("kick 120 bpm.wav")), Some(120.0));
  }

  #[test]
  fn filename_explicit_wins_over_trailing() {
    // `120bpm` matches explicit; trailing `99` would otherwise win in some readings.
    assert_eq!(
      parse_bpm_from_filename(&p("loop_120bpm_v99.wav")),
      Some(120.0)
    );
  }

  #[test]
  fn filename_out_of_range_rejected() {
    assert_eq!(parse_bpm_from_filename(&p("synth500.wav")), None);
    assert_eq!(parse_bpm_from_filename(&p("kick_30.wav")), None);
  }

  #[test]
  fn filename_no_match() {
    assert_eq!(parse_bpm_from_filename(&p("kick.wav")), None);
    assert_eq!(parse_bpm_from_filename(&p("something(128k).wav")), None);
    assert_eq!(parse_bpm_from_filename(&p("kick_001.wav")), None);
  }

  #[test]
  fn bar_count_4_4_120bpm() {
    // 8 beats @ 120 BPM 4/4 → 4 seconds total → 2 bars.
    let bc = compute_bar_count(4.0, 120.0, (4, 4));
    assert!((bc - 2.0).abs() < 1e-9);
  }

  #[test]
  fn bar_count_non_integer() {
    // 5.28s @ 120 BPM 4/4 → 5.28 / 2.0 = 2.64 bars.
    let bc = compute_bar_count(5.28, 120.0, (4, 4));
    assert!((bc - 2.64).abs() < 1e-9);
  }

  #[test]
  fn bar_count_3_4() {
    // 4s @ 90 BPM 3/4. bar_seconds = (60 * 3 * 4) / (90 * 4) = 2.0 → 2 bars.
    let bc = compute_bar_count(4.0, 90.0, (3, 4));
    assert!((bc - 2.0).abs() < 1e-9);
  }
}
