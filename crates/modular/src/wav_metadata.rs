use std::io::{Read, Seek};
use std::path::Path;

use crate::wav_bpm::{compute_bar_count, estimate_bpm_from_audio, parse_bpm_from_filename};

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackMode {
  OneShot,
  Loop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoopType {
  Forward,
  PingPong,
  Backward,
}

#[derive(Debug, Clone)]
pub struct LoopInfo {
  pub loop_type: LoopType,
  pub start_seconds: f64,
  pub end_seconds: f64,
}

#[derive(Debug, Clone)]
pub struct CuePoint {
  pub position_seconds: f64,
  pub label: String,
}

#[derive(Debug, Clone)]
pub struct WavMetadata {
  pub sample_rate: u32,
  pub frame_count: u64,
  pub bit_depth: u16,
  pub pitch: Option<f64>,
  pub playback: Option<PlaybackMode>,
  pub bpm: Option<f64>,
  pub beats: Option<u32>,
  pub time_signature: Option<(u16, u16)>,
  pub bar_count: Option<f64>,
  pub loops: Vec<LoopInfo>,
  pub cue_points: Vec<CuePoint>,
}

fn read_u16_le(data: &[u8], offset: usize) -> u16 {
  u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes([
    data[offset],
    data[offset + 1],
    data[offset + 2],
    data[offset + 3],
  ])
}

fn read_f32_le(data: &[u8], offset: usize) -> f32 {
  f32::from_le_bytes([
    data[offset],
    data[offset + 1],
    data[offset + 2],
    data[offset + 3],
  ])
}

pub fn extract<T: Read + Seek>(
  stream: &mut T,
  total_data_frames: u64,
  path: Option<&Path>,
  mono_samples: Option<&[f32]>,
) -> Result<WavMetadata, String> {
  let root = riff::Chunk::read(stream, 0).map_err(|e| format!("Failed to read RIFF: {}", e))?;

  let mut sample_rate: Option<u32> = None;
  let mut bit_depth: Option<u16> = None;
  let mut channels: Option<u16> = None;

  // smpl chunk data
  let mut smpl_pitch: Option<f64> = None;
  let mut raw_loops: Vec<(LoopType, u32, u32)> = Vec::new(); // (type, start_sample, end_sample)

  // acid chunk data
  let mut acid_pitch: Option<f64> = None;
  let mut acid_playback: Option<PlaybackMode> = None;
  let mut acid_bpm: Option<f64> = None;
  let mut acid_beats: Option<u32> = None;
  let mut acid_time_sig: Option<(u16, u16)> = None;

  // cue chunk data
  let mut cue_entries: Vec<(u32, u32)> = Vec::new(); // (id, sample_offset)
  let mut cue_labels: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

  // Collect all children first to release borrow on stream
  let children: Vec<riff::Chunk> = root
    .iter(stream)
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| format!("Failed to iterate chunks: {}", e))?;

  for child in &children {
    let id = child.id().as_str().to_string();
    match id.as_str() {
      "fmt " => {
        let data = child
          .read_contents(stream)
          .map_err(|e| format!("Failed to read fmt: {}", e))?;
        if data.len() < 16 {
          return Err("fmt chunk too short".to_string());
        }
        channels = Some(read_u16_le(&data, 2));
        sample_rate = Some(read_u32_le(&data, 4));
        bit_depth = Some(read_u16_le(&data, 14));
      }
      "smpl" => {
        let data = child
          .read_contents(stream)
          .map_err(|e| format!("Failed to read smpl: {}", e))?;
        if data.len() >= 36 {
          let midi_unity_note = read_u32_le(&data, 12);
          let midi_pitch_fraction = read_u32_le(&data, 16);
          let num_loops = read_u32_le(&data, 28);

          let fraction_cents = (midi_pitch_fraction as f64) / (4294967296.0) * 100.0;
          smpl_pitch = Some((midi_unity_note as f64 - 60.0 + fraction_cents / 100.0) / 12.0);

          for i in 0..num_loops as usize {
            let base = 36 + i * 24;
            if base + 16 > data.len() {
              break;
            }
            let loop_type_val = read_u32_le(&data, base + 4);
            let start_sample = read_u32_le(&data, base + 8);
            let end_sample = read_u32_le(&data, base + 12);

            let lt = match loop_type_val {
              1 => LoopType::PingPong,
              2 => LoopType::Backward,
              _ => LoopType::Forward,
            };

            raw_loops.push((lt, start_sample, end_sample));
          }
        }
      }
      "cue " => {
        let data = child
          .read_contents(stream)
          .map_err(|e| format!("Failed to read cue: {}", e))?;
        if data.len() >= 4 {
          let num_cues = read_u32_le(&data, 0);
          for i in 0..num_cues as usize {
            let base = 4 + i * 24;
            if base + 24 > data.len() {
              break;
            }
            let id = read_u32_le(&data, base);
            let sample_offset = read_u32_le(&data, base + 20);
            cue_entries.push((id, sample_offset));
          }
        }
      }
      "LIST" => {
        let list_type = child
          .read_type(stream)
          .map_err(|e| format!("Failed to read LIST type: {}", e))?;
        if list_type.as_str() == "adtl" {
          let sub_chunks: Vec<riff::Chunk> = child
            .iter(stream)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to iterate adtl: {}", e))?;
          for sub in &sub_chunks {
            if sub.id().as_str() == "labl" {
              let data = sub
                .read_contents(stream)
                .map_err(|e| format!("Failed to read labl: {}", e))?;
              if data.len() >= 4 {
                let cue_id = read_u32_le(&data, 0);
                let text = if data.len() > 4 {
                  let text_bytes = &data[4..];
                  let end = text_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(text_bytes.len());
                  String::from_utf8_lossy(&text_bytes[..end]).to_string()
                } else {
                  String::new()
                };
                cue_labels.insert(cue_id, text);
              }
            }
          }
        }
      }
      // Sony Acid chunk format: flags(4) root_note(2) padding(2) padding(4) beats(4) ts_num(2) ts_den(2) tempo(4)
      "acid" => {
        let data = child
          .read_contents(stream)
          .map_err(|e| format!("Failed to read acid: {}", e))?;
        if data.len() >= 24 {
          let flags = read_u32_le(&data, 0);
          let root_note = read_u16_le(&data, 4);
          let beats = read_u32_le(&data, 12);
          let ts_num = read_u16_le(&data, 16);
          let ts_den = read_u16_le(&data, 18);
          let tempo = read_f32_le(&data, 20);

          if flags & 1 != 0 {
            acid_playback = Some(PlaybackMode::OneShot);
          } else {
            acid_playback = Some(PlaybackMode::Loop);
          }

          if flags & 2 != 0 {
            acid_pitch = Some((root_note as f64 - 60.0) / 12.0);
          }

          acid_bpm = if tempo > 0.0 {
            Some(tempo as f64)
          } else {
            None
          };
          acid_beats = Some(beats);
          acid_time_sig = Some((ts_num, ts_den));
        }
      }
      _ => {}
    }
  }

  let sr = sample_rate.ok_or("Missing fmt chunk")?;
  let bd = bit_depth.ok_or("Missing fmt chunk")?;
  let _channels = channels.ok_or("Missing fmt chunk")?;

  // Convert raw loop sample offsets to seconds now that sample_rate is known
  let loops: Vec<LoopInfo> = raw_loops
    .into_iter()
    .map(|(loop_type, start, end)| LoopInfo {
      loop_type,
      start_seconds: start as f64 / sr as f64,
      end_seconds: end as f64 / sr as f64,
    })
    .collect();

  // Assemble cue points
  let cue_points: Vec<CuePoint> = cue_entries
    .iter()
    .map(|(id, sample_offset)| CuePoint {
      position_seconds: *sample_offset as f64 / sr as f64,
      label: cue_labels.get(id).cloned().unwrap_or_default(),
    })
    .collect();

  // smpl pitch overrides acid pitch
  let pitch = smpl_pitch.or(acid_pitch);

  // BPM fallback chain: ACID → filename → audio analysis.
  let bpm = acid_bpm
    .or_else(|| path.and_then(parse_bpm_from_filename))
    .or_else(|| mono_samples.and_then(|s| estimate_bpm_from_audio(s, sr)));

  let bar_count = bpm.map(|b| {
    let ts = acid_time_sig.unwrap_or((4, 4));
    let duration_seconds = total_data_frames as f64 / sr as f64;
    compute_bar_count(duration_seconds, b, ts)
  });

  Ok(WavMetadata {
    sample_rate: sr,
    frame_count: total_data_frames,
    bit_depth: bd,
    pitch,
    playback: acid_playback,
    bpm,
    beats: acid_beats,
    time_signature: acid_time_sig,
    bar_count,
    loops,
    cue_points,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Cursor;

  fn write_u16_le(buf: &mut Vec<u8>, val: u16) {
    buf.extend_from_slice(&val.to_le_bytes());
  }

  fn write_u32_le(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
  }

  fn write_f32_le(buf: &mut Vec<u8>, val: f32) {
    buf.extend_from_slice(&val.to_le_bytes());
  }

  /// Build a minimal WAV with fmt + data chunks
  fn minimal_wav(sample_rate: u32, channels: u16, bit_depth: u16, num_frames: u32) -> Vec<u8> {
    let bytes_per_sample = bit_depth / 8;
    let data_size = num_frames * channels as u32 * bytes_per_sample as u32;

    let mut buf = Vec::new();

    // RIFF header (placeholder size)
    buf.extend_from_slice(b"RIFF");
    write_u32_le(&mut buf, 0); // placeholder
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    write_u32_le(&mut buf, 16); // chunk size
    write_u16_le(&mut buf, 1); // PCM format
    write_u16_le(&mut buf, channels);
    write_u32_le(&mut buf, sample_rate);
    write_u32_le(
      &mut buf,
      sample_rate * channels as u32 * bytes_per_sample as u32,
    ); // byte rate
    write_u16_le(&mut buf, channels * bytes_per_sample); // block align
    write_u16_le(&mut buf, bit_depth);

    // data chunk
    buf.extend_from_slice(b"data");
    write_u32_le(&mut buf, data_size);
    buf.resize(buf.len() + data_size as usize, 0);

    // Fix RIFF size
    let riff_size = (buf.len() - 8) as u32;
    buf[4..8].copy_from_slice(&riff_size.to_le_bytes());

    buf
  }

  fn append_smpl_chunk(
    wav: &mut Vec<u8>,
    midi_note: u32,
    pitch_fraction: u32,
    loops: &[(u32, u32, u32)],
  ) {
    let num_loops = loops.len() as u32;
    let chunk_size = 36 + num_loops * 24;

    wav.extend_from_slice(b"smpl");
    write_u32_le(wav, chunk_size);

    // smpl header: manufacturer(4), product(4), sample_period(4), midi_unity_note(4), midi_pitch_fraction(4), smpte_format(4), smpte_offset(4), num_loops(4), sampler_data(4)
    write_u32_le(wav, 0); // manufacturer
    write_u32_le(wav, 0); // product
    write_u32_le(wav, 0); // sample_period
    write_u32_le(wav, midi_note);
    write_u32_le(wav, pitch_fraction);
    write_u32_le(wav, 0); // smpte_format
    write_u32_le(wav, 0); // smpte_offset
    write_u32_le(wav, num_loops);
    write_u32_le(wav, 0); // sampler_data

    for &(loop_type, start, end) in loops {
      write_u32_le(wav, 0); // cue point id
      write_u32_le(wav, loop_type);
      write_u32_le(wav, start);
      write_u32_le(wav, end);
      write_u32_le(wav, 0); // fraction
      write_u32_le(wav, 0); // play_count
    }

    // Fix RIFF size
    let riff_size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
  }

  fn append_cue_chunk(wav: &mut Vec<u8>, cues: &[(u32, u32)]) {
    let num_cues = cues.len() as u32;
    let chunk_size = 4 + num_cues * 24;

    wav.extend_from_slice(b"cue ");
    write_u32_le(wav, chunk_size);
    write_u32_le(wav, num_cues);

    for &(id, sample_offset) in cues {
      write_u32_le(wav, id); // id
      write_u32_le(wav, 0); // position
      write_u32_le(wav, 0); // fcc_chunk (data)
      write_u32_le(wav, 0); // chunk_start
      write_u32_le(wav, 0); // block_start
      write_u32_le(wav, sample_offset); // sample_offset
    }

    let riff_size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
  }

  fn append_adtl_chunk(wav: &mut Vec<u8>, labels: &[(u32, &str)]) {
    // Build the LIST-adtl content first
    let mut list_content = Vec::new();
    list_content.extend_from_slice(b"adtl");

    for &(cue_id, text) in labels {
      let text_bytes = text.as_bytes();
      let labl_data_size = 4 + text_bytes.len() + 1; // id + text + null
      let padded_size = if labl_data_size % 2 != 0 {
        labl_data_size + 1
      } else {
        labl_data_size
      };

      list_content.extend_from_slice(b"labl");
      write_u32_le(&mut list_content, labl_data_size as u32);
      write_u32_le(&mut list_content, cue_id);
      list_content.extend_from_slice(text_bytes);
      list_content.push(0); // null terminator
      if padded_size > labl_data_size {
        list_content.push(0); // padding byte
      }
    }

    wav.extend_from_slice(b"LIST");
    write_u32_le(wav, list_content.len() as u32);
    wav.extend_from_slice(&list_content);

    let riff_size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
  }

  fn append_acid_chunk(
    wav: &mut Vec<u8>,
    flags: u32,
    root_note: u16,
    beats: u32,
    ts_num: u16,
    ts_den: u16,
    tempo: f32,
  ) {
    wav.extend_from_slice(b"acid");
    write_u32_le(wav, 24);

    write_u32_le(wav, flags);
    write_u16_le(wav, root_note);
    write_u16_le(wav, 0); // padding
    write_u32_le(wav, 0); // padding
    write_u32_le(wav, beats);
    write_u16_le(wav, ts_num);
    write_u16_le(wav, ts_den);
    write_f32_le(wav, tempo);

    let riff_size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
  }

  #[test]
  fn test_fmt_only_wav() {
    let wav = minimal_wav(44100, 2, 16, 1000);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 1000, None, None).unwrap();

    assert_eq!(meta.sample_rate, 44100);
    assert_eq!(meta.frame_count, 1000);
    assert_eq!(meta.bit_depth, 16);
    assert!(meta.pitch.is_none());
    assert!(meta.playback.is_none());
    assert!(meta.bpm.is_none());
    assert!(meta.beats.is_none());
    assert!(meta.time_signature.is_none());
    assert!(meta.loops.is_empty());
    assert!(meta.cue_points.is_empty());
  }

  #[test]
  fn test_smpl_chunk_pitch_and_loops() {
    let mut wav = minimal_wav(44100, 1, 16, 44100);
    // MIDI 60 = C4 = 0V, two loops: forward (0-22050), pingpong (22050-44100)
    append_smpl_chunk(&mut wav, 60, 0, &[(0, 0, 22050), (1, 22050, 44100)]);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 44100, None, None).unwrap();

    assert!((meta.pitch.unwrap() - 0.0).abs() < 1e-10);
    assert_eq!(meta.loops.len(), 2);
    assert_eq!(meta.loops[0].loop_type, LoopType::Forward);
    assert!((meta.loops[0].start_seconds - 0.0).abs() < 1e-10);
    assert!((meta.loops[0].end_seconds - 0.5).abs() < 1e-6);
    assert_eq!(meta.loops[1].loop_type, LoopType::PingPong);
    assert!((meta.loops[1].start_seconds - 0.5).abs() < 1e-6);
    assert!((meta.loops[1].end_seconds - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_smpl_pitch_midi72() {
    let mut wav = minimal_wav(44100, 1, 16, 100);
    append_smpl_chunk(&mut wav, 72, 0, &[]);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 100, None, None).unwrap();
    assert!((meta.pitch.unwrap() - 1.0).abs() < 1e-10);
  }

  #[test]
  fn test_smpl_pitch_with_fine_tune() {
    let mut wav = minimal_wav(44100, 1, 16, 100);
    // MIDI 69 (A4) + 50 cents
    // fraction_cents = raw / 2^32 * 100 = 50
    // raw = 50 / 100 * 2^32 = 2147483648
    let fraction = 2147483648u32;
    append_smpl_chunk(&mut wav, 69, fraction, &[]);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 100, None, None).unwrap();

    // pitch = (69 - 60 + 50/100) / 12 = 9.5 / 12
    let expected = 9.5 / 12.0;
    assert!((meta.pitch.unwrap() - expected).abs() < 1e-6);
  }

  #[test]
  fn test_cue_and_labels() {
    let mut wav = minimal_wav(48000, 1, 16, 96000);
    // 3 cues at samples 0, 48000, 72000
    append_cue_chunk(&mut wav, &[(1, 0), (2, 48000), (3, 72000)]);
    // Labels for cues 1 and 2 only
    append_adtl_chunk(&mut wav, &[(1, "intro"), (2, "verse")]);

    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 96000, None, None).unwrap();

    assert_eq!(meta.cue_points.len(), 3);
    assert!((meta.cue_points[0].position_seconds - 0.0).abs() < 1e-10);
    assert_eq!(meta.cue_points[0].label, "intro");
    assert!((meta.cue_points[1].position_seconds - 1.0).abs() < 1e-10);
    assert_eq!(meta.cue_points[1].label, "verse");
    assert!((meta.cue_points[2].position_seconds - 1.5).abs() < 1e-6);
    assert_eq!(meta.cue_points[2].label, "");
  }

  #[test]
  fn test_acid_chunk() {
    let mut wav = minimal_wav(44100, 1, 16, 100);
    // flags=2 (root note valid, loop mode), root=60, beats=8, 4/4, 120bpm
    append_acid_chunk(&mut wav, 2, 60, 8, 4, 4, 120.0);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 100, None, None).unwrap();

    assert!((meta.bpm.unwrap() - 120.0).abs() < 1e-6);
    assert_eq!(meta.beats.unwrap(), 8);
    assert_eq!(meta.time_signature.unwrap(), (4, 4));
    assert_eq!(meta.playback.unwrap(), PlaybackMode::Loop);
  }

  #[test]
  fn test_acid_one_shot() {
    let mut wav = minimal_wav(44100, 1, 16, 100);
    // flags=1 (one-shot)
    append_acid_chunk(&mut wav, 1, 0, 0, 4, 4, 0.0);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 100, None, None).unwrap();
    assert_eq!(meta.playback.unwrap(), PlaybackMode::OneShot);
    assert!(meta.bpm.is_none()); // zero tempo filtered out
  }

  #[test]
  fn test_acid_root_note_as_pitch_fallback() {
    let mut wav = minimal_wav(44100, 1, 16, 100);
    // flags=2 (root note valid), root=72 => pitch = (72-60)/12 = 1.0V
    append_acid_chunk(&mut wav, 2, 72, 4, 4, 4, 140.0);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 100, None, None).unwrap();
    assert!((meta.pitch.unwrap() - 1.0).abs() < 1e-10);
  }

  #[test]
  fn test_smpl_pitch_overrides_acid() {
    let mut wav = minimal_wav(44100, 1, 16, 100);
    // smpl: MIDI 60 = 0V
    append_smpl_chunk(&mut wav, 60, 0, &[]);
    // acid: root 72 = 1V (should be ignored)
    append_acid_chunk(&mut wav, 2, 72, 4, 4, 4, 120.0);
    let mut cursor = Cursor::new(wav);
    let meta = extract(&mut cursor, 100, None, None).unwrap();
    assert!((meta.pitch.unwrap() - 0.0).abs() < 1e-10);
  }
}
