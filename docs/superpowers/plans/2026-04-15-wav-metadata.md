# WAV Metadata Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract WAV metadata (format info, sampler loops, cue points, ACID chunk) from WAV files and expose it on the `WavHandle` in the DSL.

**Architecture:** Add a `wav_metadata.rs` module that uses the `riff` crate for RIFF chunk traversal and manual binary parsing of `fmt `, `smpl`, `cue `, `adtl`/`labl`, and `acid` chunks. The metadata flows through the existing `WavLoadInfo` N-API struct to the TypeScript DSL layer where it's exposed on the wav handle object.

**Tech Stack:** Rust (`riff` crate for chunk traversal, manual binary parsing), N-API, TypeScript

---

## File Structure

| File                                      | Action     | Responsibility                                                                                            |
| ----------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------- |
| `crates/modular/Cargo.toml`               | Modify     | Add `riff = "2"` dependency                                                                               |
| `crates/modular/src/wav_metadata.rs`      | Create     | Metadata extraction: parse `fmt `, `smpl`, `cue `/`adtl`, `acid` chunks                                   |
| `crates/modular/src/lib.rs`               | Modify     | Expand `WavLoadInfo` struct, call `wav_metadata::extract()` in `WavCache::load()`, add `mod wav_metadata` |
| `src/main/dsl/executor.ts`                | Modify     | Expand wav handle object with all metadata fields                                                         |
| `src/main/dsl/typescriptLibGen.ts`        | Modify     | Update `WavHandle` type definition                                                                        |
| `crates/modular/index.d.ts`               | Regenerate | Run `yarn generate-lib`                                                                                   |
| `src/main/dsl/__tests__/executor.test.ts` | Modify     | Add tests for metadata fields on wav handle                                                               |

---

### Task 1: Add `riff` dependency

**Files:**

- Modify: `crates/modular/Cargo.toml:17`

- [ ] **Step 1: Add riff dependency**

In `crates/modular/Cargo.toml`, add `riff = "2"` after the `hound` line (line 17):

```toml
hound = "3.5"
riff = "2"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p modular`
Expected: Compiles successfully (riff has zero dependencies)

- [ ] **Step 3: Commit**

```bash
git add crates/modular/Cargo.toml
git commit -m "feat(wav-metadata): add riff crate dependency"
```

---

### Task 2: Create `wav_metadata.rs` with types and `fmt ` parsing

**Files:**

- Create: `crates/modular/src/wav_metadata.rs`

- [ ] **Step 1: Write the failing test for fmt chunk parsing**

Create `crates/modular/src/wav_metadata.rs` with the types and a test:

```rust
use std::io::{Cursor, Read, Seek};

/// Metadata extracted from a WAV file's RIFF chunks.
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
    pub loops: Vec<LoopInfo>,
    pub cue_points: Vec<CuePoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackMode {
    OneShot,
    Loop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopInfo {
    pub loop_type: LoopType,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopType {
    Forward,
    PingPong,
    Backward,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CuePoint {
    pub position_seconds: f64,
    pub label: String,
}

/// Helper: read a little-endian u16 from a byte slice at offset.
fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
}

/// Helper: read a little-endian u32 from a byte slice at offset.
fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Helper: read a little-endian f32 from a byte slice at offset.
fn read_f32_le(data: &[u8], offset: usize) -> Option<f32> {
    data.get(offset..offset + 4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Extract metadata from a WAV file's RIFF chunks.
///
/// `total_data_frames` is the number of audio frames (from hound or similar),
/// needed because the `data` chunk size can be unreliable for large files.
pub fn extract<T: Read + Seek>(stream: &mut T, total_data_frames: u64) -> Result<WavMetadata, String> {
    let root = riff::Chunk::read(stream, 0)
        .map_err(|e| format!("Failed to read RIFF header: {e}"))?;

    let mut sample_rate: u32 = 0;
    let mut bit_depth: u16 = 0;
    let mut channels: u16 = 0;
    let mut pitch: Option<f64> = None;
    let mut playback: Option<PlaybackMode> = None;
    let mut bpm: Option<f64> = None;
    let mut beats: Option<u32> = None;
    let mut time_signature: Option<(u16, u16)> = None;
    let mut loops: Vec<LoopInfo> = Vec::new();
    let mut cue_points: Vec<CuePoint> = Vec::new();
    let mut cue_sample_offsets: Vec<(u32, u32)> = Vec::new(); // (id, sample_offset)
    let mut labels: Vec<(u32, String)> = Vec::new(); // (cue_point_id, text)

    for child in root.iter(stream) {
        let id_str = child.id().as_str();
        match id_str {
            "fmt " => {
                let data = child.read_contents(stream)
                    .map_err(|e| format!("Failed to read fmt chunk: {e}"))?;
                if data.len() < 16 {
                    return Err("fmt chunk too short".to_string());
                }
                channels = read_u16_le(&data, 2).unwrap();
                sample_rate = read_u32_le(&data, 4).unwrap();
                bit_depth = read_u16_le(&data, 14).unwrap();
            }
            _ => {}
        }
    }

    if sample_rate == 0 {
        return Err("No fmt chunk found".to_string());
    }

    Ok(WavMetadata {
        sample_rate,
        frame_count: total_data_frames,
        bit_depth,
        pitch,
        playback,
        bpm,
        beats,
        time_signature,
        loops,
        cue_points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal valid WAV file in memory with only a fmt chunk.
    fn minimal_wav(sample_rate: u32, channels: u16, bit_depth: u16, num_frames: u32) -> Vec<u8> {
        let data_size = num_frames * channels as u32 * (bit_depth as u32 / 8);
        let fmt_chunk_size: u32 = 16;
        // RIFF header (12) + fmt chunk (8 + 16) + data chunk (8 + data_size)
        let riff_size = 4 + (8 + fmt_chunk_size) + (8 + data_size);

        let mut buf = Vec::new();
        // RIFF header
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&riff_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        // fmt chunk
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&fmt_chunk_size.to_le_bytes());
        let audio_format: u16 = 1; // PCM
        buf.extend_from_slice(&audio_format.to_le_bytes());
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        let byte_rate = sample_rate * channels as u32 * (bit_depth as u32 / 8);
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        let block_align = channels * (bit_depth / 8);
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bit_depth.to_le_bytes());
        // data chunk
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        buf.resize(buf.len() + data_size as usize, 0); // silence

        buf
    }

    #[test]
    fn test_fmt_only_wav() {
        let wav = minimal_wav(44100, 2, 16, 1000);
        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 1000).unwrap();
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
}
```

- [ ] **Step 2: Register the module in lib.rs**

Add `mod wav_metadata;` to `crates/modular/src/lib.rs` after line 7 (after `mod validation;`):

```rust
mod validation;
mod wav_metadata;
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p modular test_fmt_only_wav`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/modular/src/wav_metadata.rs crates/modular/src/lib.rs
git commit -m "feat(wav-metadata): add wav_metadata module with fmt chunk parsing"
```

---

### Task 3: Add `smpl` chunk parsing (pitch + loops)

**Files:**

- Modify: `crates/modular/src/wav_metadata.rs`

- [ ] **Step 1: Write the failing test for smpl chunk**

Add to the `tests` module in `wav_metadata.rs`:

```rust
    /// Append a smpl chunk to an existing WAV buffer (before closing RIFF size fixup).
    fn append_smpl_chunk(wav: &mut Vec<u8>, midi_note: u32, midi_pitch_fraction: u32, loops: &[(u32, u32, u32)]) {
        // loops: [(type, start_sample, end_sample), ...]
        let num_loops = loops.len() as u32;
        let chunk_size: u32 = 36 + num_loops * 24; // header(36) + loops(24 each)

        let chunk_start = wav.len();
        wav.extend_from_slice(b"smpl");
        wav.extend_from_slice(&chunk_size.to_le_bytes());
        wav.extend_from_slice(&0u32.to_le_bytes()); // manufacturer
        wav.extend_from_slice(&0u32.to_le_bytes()); // product
        let sample_period = 1_000_000_000u32 / 44100; // nanoseconds per sample
        wav.extend_from_slice(&sample_period.to_le_bytes());
        wav.extend_from_slice(&midi_note.to_le_bytes());
        wav.extend_from_slice(&midi_pitch_fraction.to_le_bytes());
        wav.extend_from_slice(&0u32.to_le_bytes()); // smpte_format
        wav.extend_from_slice(&0u32.to_le_bytes()); // smpte_offset
        wav.extend_from_slice(&num_loops.to_le_bytes());
        wav.extend_from_slice(&0u32.to_le_bytes()); // sampler_data_size

        for (i, (loop_type, start, end)) in loops.iter().enumerate() {
            wav.extend_from_slice(&(i as u32).to_le_bytes()); // cue_point_id
            wav.extend_from_slice(&loop_type.to_le_bytes());
            wav.extend_from_slice(&start.to_le_bytes());
            wav.extend_from_slice(&end.to_le_bytes());
            wav.extend_from_slice(&0u32.to_le_bytes()); // fraction
            wav.extend_from_slice(&0u32.to_le_bytes()); // play_count
        }

        // Fix RIFF size
        let riff_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
    }

    #[test]
    fn test_smpl_chunk_pitch_and_loops() {
        let mut wav = minimal_wav(44100, 1, 16, 44100); // 1 second at 44100
        // MIDI note 60 = C4 = 0V, no fine tune
        append_smpl_chunk(&mut wav, 60, 0, &[
            (0, 0, 22050),     // forward loop, 0s to 0.5s
            (1, 11025, 33075), // pingpong loop, 0.25s to 0.75s
        ]);

        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 44100).unwrap();

        // Pitch: MIDI 60 = 0V
        assert!((meta.pitch.unwrap() - 0.0).abs() < 1e-9);

        assert_eq!(meta.loops.len(), 2);
        assert_eq!(meta.loops[0].loop_type, LoopType::Forward);
        assert!((meta.loops[0].start_seconds - 0.0).abs() < 1e-9);
        assert!((meta.loops[0].end_seconds - 0.5).abs() < 1e-6);
        assert_eq!(meta.loops[1].loop_type, LoopType::PingPong);
        assert!((meta.loops[1].start_seconds - 0.25).abs() < 1e-6);
        assert!((meta.loops[1].end_seconds - 0.75).abs() < 1e-6);
    }

    #[test]
    fn test_smpl_pitch_midi72() {
        let mut wav = minimal_wav(44100, 1, 16, 100);
        append_smpl_chunk(&mut wav, 72, 0, &[]);
        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 100).unwrap();
        // MIDI 72 = C5 = 1V
        assert!((meta.pitch.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_smpl_pitch_with_fine_tune() {
        let mut wav = minimal_wav(44100, 1, 16, 100);
        // MIDI 69 (A4) with 50 cents fine tune
        // midi_pitch_fraction for 50 cents: 50/100 * 2^32 = 2147483648
        let fraction = (50.0 / 100.0 * (u32::MAX as f64 + 1.0)) as u32;
        append_smpl_chunk(&mut wav, 69, fraction, &[]);
        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 100).unwrap();
        // Expected: (69 - 60 + 50/100) / 12 = 9.5 / 12
        let expected = (69.0 - 60.0 + 0.5) / 12.0;
        assert!((meta.pitch.unwrap() - expected).abs() < 1e-6);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular test_smpl`
Expected: FAIL — smpl chunk is not parsed yet (tests pass with empty optionals since the match arm doesn't exist)

Actually, the tests will fail because `smpl` is not matched — pitch will be `None` and loops will be empty. The assertions on `.unwrap()` will panic.

- [ ] **Step 3: Implement smpl chunk parsing**

Add the `"smpl"` match arm in the `extract()` function, inside the `for child in root.iter(stream)` loop:

```rust
            "smpl" => {
                let data = child.read_contents(stream)
                    .map_err(|e| format!("Failed to read smpl chunk: {e}"))?;
                if data.len() < 36 {
                    continue; // malformed, skip
                }
                let midi_unity_note = read_u32_le(&data, 12).unwrap();
                let midi_pitch_fraction_raw = read_u32_le(&data, 16).unwrap();
                let num_sample_loops = read_u32_le(&data, 28).unwrap();

                // Convert pitch fraction to cents (0-99.99...)
                let fraction_cents = midi_pitch_fraction_raw as f64 / (u32::MAX as f64 + 1.0) * 100.0;
                pitch = Some((midi_unity_note as f64 - 60.0 + fraction_cents / 100.0) / 12.0);

                // Parse loops
                let mut offset = 36;
                for _ in 0..num_sample_loops {
                    if offset + 24 > data.len() {
                        break;
                    }
                    let loop_type_raw = read_u32_le(&data, offset + 4).unwrap();
                    let start_sample = read_u32_le(&data, offset + 8).unwrap();
                    let end_sample = read_u32_le(&data, offset + 12).unwrap();

                    let loop_type = match loop_type_raw {
                        1 => LoopType::PingPong,
                        2 => LoopType::Backward,
                        _ => LoopType::Forward, // 0 or unknown = forward
                    };

                    loops.push(LoopInfo {
                        loop_type,
                        start_seconds: start_sample as f64 / sample_rate as f64,
                        end_seconds: end_sample as f64 / sample_rate as f64,
                    });

                    offset += 24;
                }
            }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p modular test_smpl`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add crates/modular/src/wav_metadata.rs
git commit -m "feat(wav-metadata): parse smpl chunk for pitch and loops"
```

---

### Task 4: Add `cue ` + `adtl`/`labl` chunk parsing

**Files:**

- Modify: `crates/modular/src/wav_metadata.rs`

- [ ] **Step 1: Write the failing test for cue + label parsing**

Add to the `tests` module:

```rust
    /// Append a cue chunk to an existing WAV buffer.
    fn append_cue_chunk(wav: &mut Vec<u8>, cues: &[(u32, u32)]) {
        // cues: [(id, sample_offset), ...]
        let num_cues = cues.len() as u32;
        let chunk_size: u32 = 4 + num_cues * 24;

        wav.extend_from_slice(b"cue ");
        wav.extend_from_slice(&chunk_size.to_le_bytes());
        wav.extend_from_slice(&num_cues.to_le_bytes());

        for (id, sample_offset) in cues {
            wav.extend_from_slice(&id.to_le_bytes());         // id
            wav.extend_from_slice(&sample_offset.to_le_bytes()); // position
            wav.extend_from_slice(b"data");                    // data_chunk_id
            wav.extend_from_slice(&0u32.to_le_bytes());        // chunk_start
            wav.extend_from_slice(&0u32.to_le_bytes());        // block_start
            wav.extend_from_slice(&sample_offset.to_le_bytes()); // sample_offset
        }

        // Fix RIFF size
        let riff_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
    }

    /// Append a LIST/adtl chunk with labl sub-chunks.
    fn append_adtl_chunk(wav: &mut Vec<u8>, labels: &[(u32, &str)]) {
        // labels: [(cue_point_id, text), ...]
        // Calculate total size of all labl sub-chunks
        let mut labl_total = 0u32;
        for (_, text) in labels {
            let text_bytes = text.len() as u32 + 1; // null terminator
            let padded = if text_bytes % 2 != 0 { text_bytes + 1 } else { text_bytes };
            labl_total += 8 + 4 + padded; // chunk header(8) + cue_id(4) + text
        }
        let list_data_size = 4 + labl_total; // "adtl" + labl chunks

        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&list_data_size.to_le_bytes());
        wav.extend_from_slice(b"adtl");

        for (cue_id, text) in labels {
            let text_bytes = text.len() as u32 + 1; // null terminator
            let padded = if text_bytes % 2 != 0 { text_bytes + 1 } else { text_bytes };
            let labl_size = 4 + padded; // cue_id + text
            wav.extend_from_slice(b"labl");
            wav.extend_from_slice(&labl_size.to_le_bytes());
            wav.extend_from_slice(&cue_id.to_le_bytes());
            wav.extend_from_slice(text.as_bytes());
            wav.push(0); // null terminator
            if text_bytes % 2 != 0 {
                wav.push(0); // pad byte
            }
        }

        // Fix RIFF size
        let riff_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
    }

    #[test]
    fn test_cue_and_labels() {
        let mut wav = minimal_wav(48000, 1, 16, 48000); // 1 second at 48kHz
        append_cue_chunk(&mut wav, &[
            (1, 0),     // cue at 0s
            (2, 24000), // cue at 0.5s
            (3, 36000), // cue at 0.75s
        ]);
        append_adtl_chunk(&mut wav, &[
            (1, "Intro"),
            (2, "Verse"),
            // cue 3 has no label
        ]);

        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 48000).unwrap();

        assert_eq!(meta.cue_points.len(), 3);
        assert!((meta.cue_points[0].position_seconds - 0.0).abs() < 1e-9);
        assert_eq!(meta.cue_points[0].label, "Intro");
        assert!((meta.cue_points[1].position_seconds - 0.5).abs() < 1e-6);
        assert_eq!(meta.cue_points[1].label, "Verse");
        assert!((meta.cue_points[2].position_seconds - 0.75).abs() < 1e-6);
        assert_eq!(meta.cue_points[2].label, ""); // no label
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular test_cue_and_labels`
Expected: FAIL — cue points not parsed

- [ ] **Step 3: Implement cue + adtl/labl parsing**

Add these match arms in the `extract()` function's chunk iteration loop:

```rust
            "cue " => {
                let data = child.read_contents(stream)
                    .map_err(|e| format!("Failed to read cue chunk: {e}"))?;
                if data.len() < 4 {
                    continue;
                }
                let num_cues = read_u32_le(&data, 0).unwrap();
                let mut offset = 4;
                for _ in 0..num_cues {
                    if offset + 24 > data.len() {
                        break;
                    }
                    let id = read_u32_le(&data, offset).unwrap();
                    let sample_offset = read_u32_le(&data, offset + 20).unwrap();
                    cue_sample_offsets.push((id, sample_offset));
                    offset += 24;
                }
            }
            "LIST" => {
                let list_type = child.read_type(stream)
                    .map_err(|e| format!("Failed to read LIST type: {e}"))?;
                if list_type.as_str() == "adtl" {
                    for sub_chunk in child.iter(stream) {
                        if sub_chunk.id().as_str() == "labl" {
                            let data = sub_chunk.read_contents(stream)
                                .map_err(|e| format!("Failed to read labl chunk: {e}"))?;
                            if data.len() < 5 {
                                continue;
                            }
                            let cue_id = read_u32_le(&data, 0).unwrap();
                            // Text starts at offset 4, null-terminated
                            let text_bytes = &data[4..];
                            let text = text_bytes.split(|&b| b == 0)
                                .next()
                                .map(|b| String::from_utf8_lossy(b).to_string())
                                .unwrap_or_default();
                            labels.push((cue_id, text));
                        }
                    }
                }
            }
```

Then, **after** the chunk iteration loop (before the `Ok(...)` return), assemble cue points from the collected offsets and labels:

```rust
    // Assemble cue points from collected data
    for (id, sample_offset) in &cue_sample_offsets {
        let label = labels.iter()
            .find(|(cue_id, _)| cue_id == id)
            .map(|(_, text)| text.clone())
            .unwrap_or_default();
        cue_points.push(CuePoint {
            position_seconds: *sample_offset as f64 / sample_rate as f64,
            label,
        });
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p modular test_cue_and_labels`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/modular/src/wav_metadata.rs
git commit -m "feat(wav-metadata): parse cue and adtl/labl chunks"
```

---

### Task 5: Add `acid` chunk parsing

**Files:**

- Modify: `crates/modular/src/wav_metadata.rs`

- [ ] **Step 1: Write the failing test for acid chunk**

Add to the `tests` module:

```rust
    /// Append an acid chunk to an existing WAV buffer.
    fn append_acid_chunk(wav: &mut Vec<u8>, flags: u32, root_note: u16, beats: u32, numerator: u16, denominator: u16, tempo: f32) {
        let chunk_size: u32 = 24; // acid chunk is always 24 bytes

        wav.extend_from_slice(b"acid");
        wav.extend_from_slice(&chunk_size.to_le_bytes());
        wav.extend_from_slice(&flags.to_le_bytes());       // flags
        wav.extend_from_slice(&root_note.to_le_bytes());    // root note
        wav.extend_from_slice(&0u16.to_le_bytes());         // unknown
        wav.extend_from_slice(&0f32.to_le_bytes());         // unknown
        wav.extend_from_slice(&beats.to_le_bytes());        // beats
        wav.extend_from_slice(&numerator.to_le_bytes());    // time sig numerator
        wav.extend_from_slice(&denominator.to_le_bytes());  // time sig denominator (power of 2)
        wav.extend_from_slice(&tempo.to_le_bytes());        // BPM

        // Fix RIFF size
        let riff_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
    }

    #[test]
    fn test_acid_chunk() {
        let mut wav = minimal_wav(44100, 2, 16, 44100);
        // flags: bit 0 = one-shot (0x01), bit 1 = root note valid (0x02)
        // one-shot = false (not set), root note valid = true (0x02)
        append_acid_chunk(&mut wav, 0x02, 69, 16, 4, 4, 120.0);

        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 44100).unwrap();

        assert_eq!(meta.playback, Some(PlaybackMode::Loop)); // not one-shot
        assert_eq!(meta.bpm, Some(120.0));
        assert_eq!(meta.beats, Some(16));
        assert_eq!(meta.time_signature, Some((4, 4)));
    }

    #[test]
    fn test_acid_one_shot() {
        let mut wav = minimal_wav(44100, 1, 16, 100);
        // flags: bit 0 set = one-shot
        append_acid_chunk(&mut wav, 0x01, 0, 0, 4, 4, 0.0);

        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 100).unwrap();

        assert_eq!(meta.playback, Some(PlaybackMode::OneShot));
    }

    #[test]
    fn test_acid_root_note_as_pitch_fallback() {
        let mut wav = minimal_wav(44100, 1, 16, 100);
        // No smpl chunk, acid with root note valid (flag 0x02), root = 72 (C5)
        append_acid_chunk(&mut wav, 0x02, 72, 4, 4, 4, 140.0);

        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 100).unwrap();

        // No smpl chunk, so pitch from acid: (72 - 60) / 12 = 1.0V
        assert!((meta.pitch.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_smpl_pitch_overrides_acid() {
        let mut wav = minimal_wav(44100, 1, 16, 100);
        append_smpl_chunk(&mut wav, 60, 0, &[]); // pitch = 0V from smpl
        append_acid_chunk(&mut wav, 0x02, 72, 4, 4, 4, 140.0); // root = 72 from acid

        let mut cursor = Cursor::new(&wav);
        let meta = extract(&mut cursor, 100).unwrap();

        // smpl pitch (0V) should take priority over acid (1V)
        assert!((meta.pitch.unwrap() - 0.0).abs() < 1e-9);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular test_acid`
Expected: FAIL

- [ ] **Step 3: Implement acid chunk parsing**

Add a new variable before the loop to track acid root note:

```rust
    let mut acid_root_note: Option<u16> = None;
```

Add the `"acid"` match arm in the chunk iteration:

```rust
            "acid" => {
                let data = child.read_contents(stream)
                    .map_err(|e| format!("Failed to read acid chunk: {e}"))?;
                if data.len() < 24 {
                    continue;
                }
                let flags = read_u32_le(&data, 0).unwrap();
                let root_note = read_u16_le(&data, 4).unwrap();
                let beat_count = read_u32_le(&data, 12).unwrap();
                let numerator = read_u16_le(&data, 16).unwrap();
                let denominator = read_u16_le(&data, 18).unwrap();
                let tempo = read_f32_le(&data, 20).unwrap();

                let is_one_shot = flags & 0x01 != 0;
                playback = Some(if is_one_shot { PlaybackMode::OneShot } else { PlaybackMode::Loop });

                if tempo > 0.0 {
                    bpm = Some(tempo as f64);
                }
                if beat_count > 0 {
                    beats = Some(beat_count);
                }
                if numerator > 0 && denominator > 0 {
                    time_signature = Some((numerator, denominator));
                }

                let root_note_valid = flags & 0x02 != 0;
                if root_note_valid {
                    acid_root_note = Some(root_note);
                }
            }
```

After the loop, before assembling cue points, add acid root note fallback:

```rust
    // If no smpl chunk set pitch, use acid root note as fallback
    if pitch.is_none() {
        if let Some(root) = acid_root_note {
            pitch = Some((root as f64 - 60.0) / 12.0);
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p modular test_acid`
Expected: ALL PASS

Also run: `cargo test -p modular test_smpl_pitch_overrides_acid`
Expected: PASS

- [ ] **Step 5: Run all wav_metadata tests**

Run: `cargo test -p modular wav_metadata`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add crates/modular/src/wav_metadata.rs
git commit -m "feat(wav-metadata): parse acid chunk for BPM, beats, time sig, playback"
```

---

### Task 6: Expand `WavLoadInfo` and integrate metadata in `WavCache::load()`

**Files:**

- Modify: `crates/modular/src/lib.rs:186-191` (WavLoadInfo struct)
- Modify: `crates/modular/src/lib.rs:81-183` (WavCache::load method)

- [ ] **Step 1: Expand the `WavLoadInfo` N-API struct**

Replace the `WavLoadInfo` struct at `crates/modular/src/lib.rs:186-191` with:

```rust
#[napi(object)]
pub struct WavLoopInfo {
    pub loop_type: String,    // "forward" | "pingpong" | "backward"
    pub start: f64,           // seconds
    pub end: f64,             // seconds
}

#[napi(object)]
pub struct WavCuePointInfo {
    pub position: f64,        // seconds
    pub label: String,
}

#[napi(object)]
pub struct WavTimeSignature {
    pub num: u32,
    pub den: u32,
}

#[napi(object)]
pub struct WavLoadInfo {
    pub channels: u32,
    pub frame_count: u32,
    pub path: String,
    pub sample_rate: u32,
    pub duration: f64,
    pub bit_depth: u32,
    pub pitch: Option<f64>,
    pub playback: Option<String>,    // "one-shot" | "loop"
    pub bpm: Option<f64>,
    pub beats: Option<u32>,
    pub time_signature: Option<WavTimeSignature>,
    pub loops: Vec<WavLoopInfo>,
    pub cue_points: Vec<WavCuePointInfo>,
}
```

- [ ] **Step 2: Update `WavCache::load()` to extract metadata**

At the top of the `load` method (after opening the file with hound), add metadata extraction. Replace the section from line 104 (`// Cache miss or stale — decode`) through the `let info = WavLoadInfo { ... }` block.

After the hound reader is opened and `spec` is read (lines 105-115), add:

```rust
        // Extract metadata from RIFF chunks
        let mut riff_file = std::fs::File::open(&full_path).map_err(|e| {
            napi::Error::from_reason(format!(
                "Failed to open WAV for metadata: {} ({})",
                full_path.display(), e
            ))
        })?;
```

After computing `frame_count` (line 162), extract metadata:

```rust
        let metadata = wav_metadata::extract(&mut riff_file, total_frames as u64)
            .map_err(|e| napi::Error::from_reason(format!(
                "Failed to extract WAV metadata from {}: {}",
                full_path.display(), e
            )))?;
```

Where `total_frames` is `raw_samples.len() / num_channels.max(1)` (the pre-resampling frame count from hound).

Update both `WavLoadInfo` construction sites (cache hit at line 96 and cache miss at line 168). For the cache hit, you'll need to store the metadata in `WavCacheEntry`. Add a `metadata: WavMetadata` field to `WavCacheEntry`:

```rust
struct WavCacheEntry {
    data: Arc<modular_core::types::WavData>,
    mtime: SystemTime,
    metadata: wav_metadata::WavMetadata,
}
```

Import the types:

```rust
use crate::wav_metadata::{WavMetadata, PlaybackMode, LoopType};
```

Create a helper function to convert `WavMetadata` to the N-API fields:

```rust
fn build_wav_load_info(rel_path: &str, num_channels: u32, frame_count: u32, meta: &wav_metadata::WavMetadata) -> WavLoadInfo {
    WavLoadInfo {
        channels: num_channels,
        frame_count,
        path: rel_path.to_string(),
        sample_rate: meta.sample_rate,
        duration: meta.frame_count as f64 / meta.sample_rate as f64,
        bit_depth: meta.bit_depth as u32,
        pitch: meta.pitch,
        playback: meta.playback.map(|p| match p {
            wav_metadata::PlaybackMode::OneShot => "one-shot".to_string(),
            wav_metadata::PlaybackMode::Loop => "loop".to_string(),
        }),
        bpm: meta.bpm,
        beats: meta.beats,
        time_signature: meta.time_signature.map(|(num, den)| WavTimeSignature {
            num: num as u32,
            den: den as u32,
        }),
        loops: meta.loops.iter().map(|l| WavLoopInfo {
            loop_type: match l.loop_type {
                wav_metadata::LoopType::Forward => "forward".to_string(),
                wav_metadata::LoopType::PingPong => "pingpong".to_string(),
                wav_metadata::LoopType::Backward => "backward".to_string(),
            },
            start: l.start_seconds,
            end: l.end_seconds,
        }).collect(),
        cue_points: meta.cue_points.iter().map(|c| WavCuePointInfo {
            position: c.position_seconds,
            label: c.label.clone(),
        }).collect(),
    }
}
```

Update cache hit return:

```rust
        if let Some(entry) = self.entries.get(rel_path) {
            if entry.mtime == mtime {
                return Ok(build_wav_load_info(
                    rel_path,
                    entry.data.channel_count() as u32,
                    entry.data.frame_count() as u32,
                    &entry.metadata,
                ));
            }
        }
```

Update cache miss — compute `total_frames` before resampling:

```rust
        let total_frames = raw_samples.len() / num_channels.max(1);
```

(This line already exists at line 134 as the variable name is `total_frames`. It gets shadowed later by `frame_count` after resampling at line 162.)

After opening `riff_file` and after computing `total_frames`, extract metadata:

```rust
        let metadata = wav_metadata::extract(&mut riff_file, total_frames as u64)
            .map_err(|e| napi::Error::from_reason(format!(
                "Failed to extract WAV metadata from {}: {}",
                full_path.display(), e
            )))?;
```

Update cache miss WavLoadInfo construction and cache insertion:

```rust
        let info = build_wav_load_info(rel_path, num_channels as u32, frame_count as u32, &metadata);

        self.entries.insert(
            rel_path.to_string(),
            WavCacheEntry {
                data: wav_data,
                mtime,
                metadata,
            },
        );

        Ok(info)
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p modular`
Expected: Compiles (there will be TypeScript type mismatches, but Rust should compile)

- [ ] **Step 4: Commit**

```bash
git add crates/modular/src/lib.rs
git commit -m "feat(wav-metadata): expand WavLoadInfo and integrate metadata extraction"
```

---

### Task 7: Update TypeScript DSL layer

**Files:**

- Modify: `src/main/dsl/executor.ts:61-69` (DSLExecutionOptions loadWav type)
- Modify: `src/main/dsl/executor.ts:334-339` (wav handle object construction)
- Modify: `src/main/dsl/typescriptLibGen.ts:318-322` (WavHandle type)

- [ ] **Step 1: Update `DSLExecutionOptions` loadWav return type**

In `src/main/dsl/executor.ts`, update the `loadWav` return type in `DSLExecutionOptions` (lines 65-69):

```typescript
    loadWav?: (path: string) => {
        channels: number;
        frameCount: number;
        path: string;
        sampleRate: number;
        duration: number;
        bitDepth: number;
        pitch?: number | null;
        playback?: string | null;
        bpm?: number | null;
        beats?: number | null;
        timeSignature?: { num: number; den: number } | null;
        loops: Array<{ loopType: string; start: number; end: number }>;
        cuePoints: Array<{ position: number; label: string }>;
    };
```

- [ ] **Step 2: Update wav handle object construction**

In `src/main/dsl/executor.ts`, update the object returned by `makeProxy` when `child === 'file'` (lines 335-339):

```typescript
const info = options.loadWav(relPath);
return {
    type: 'wav_ref' as const,
    path: relPath,
    channels: info.channels,
    sampleRate: info.sampleRate,
    frameCount: info.frameCount,
    duration: info.duration,
    bitDepth: info.bitDepth,
    ...(info.pitch != null && { pitch: info.pitch }),
    ...(info.playback != null && { playback: info.playback }),
    ...(info.bpm != null && { bpm: info.bpm }),
    ...(info.beats != null && { beats: info.beats }),
    ...(info.timeSignature != null && {
        timeSignature: {
            num: info.timeSignature.num,
            den: info.timeSignature.den,
        },
    }),
    loops: info.loops.map(
        (l: { loopType: string; start: number; end: number }) => ({
            type: l.loopType as 'forward' | 'pingpong' | 'backward',
            start: l.start,
            end: l.end,
        }),
    ),
    cuePoints: info.cuePoints.map((c: { position: number; label: string }) => ({
        position: c.position,
        label: c.label,
    })),
};
```

- [ ] **Step 3: Update WavHandle type definition for Monaco**

In `src/main/dsl/typescriptLibGen.ts`, replace the `WavHandle` type (lines 318-322):

```typescript
type WavHandle = {
    readonly type: 'wav_ref';
    readonly path: string;
    readonly channels: number;
    readonly sampleRate: number;
    readonly frameCount: number;
    readonly duration: number;
    readonly bitDepth: number;
    readonly pitch?: number;
    readonly playback?: 'one-shot' | 'loop';
    readonly bpm?: number;
    readonly beats?: number;
    readonly timeSignature?: {
        readonly num: number;
        readonly den: number;
    };
    readonly loops: ReadonlyArray<{
        readonly type: 'forward' | 'pingpong' | 'backward';
        readonly start: number;
        readonly end: number;
    }>;
    readonly cuePoints: ReadonlyArray<{
        readonly position: number;
        readonly label: string;
    }>;
};
```

- [ ] **Step 4: Verify TypeScript compiles**

Run: `yarn typecheck`
Expected: PASS (or only pre-existing errors unrelated to our changes)

- [ ] **Step 5: Commit**

```bash
git add src/main/dsl/executor.ts src/main/dsl/typescriptLibGen.ts
git commit -m "feat(wav-metadata): expose all metadata fields in DSL WavHandle"
```

---

### Task 8: Update TypeScript tests

**Files:**

- Modify: `src/main/dsl/__tests__/executor.test.ts`

- [ ] **Step 1: Update mock `loadWav` to return metadata fields**

In `src/main/dsl/__tests__/executor.test.ts`, update the `loadWav` mock (around line 758):

```typescript
const loadWav = (path: string) => ({
    channels: path === 'kick' ? 1 : 2,
    frameCount: 1000,
    path,
    sampleRate: 44100,
    duration: 1000 / 44100,
    bitDepth: 16,
    pitch: path === 'kick' ? 0.0 : null,
    playback: path === 'kick' ? 'one-shot' : null,
    bpm: null,
    beats: null,
    timeSignature: null,
    loops: [],
    cuePoints: [],
});
```

- [ ] **Step 2: Update existing test assertions**

Update the `$wavs() returns wav_ref for known files` test (line 776-780) to include new fields:

```typescript
test('$wavs() returns wav_ref for known files', () => {
    const result = execWithWavs('$sampler($wavs().kick, 5).out()');
    const sampler = findModules(result.patch, '$sampler');
    expect(sampler.length).toBe(1);
    expect(sampler[0].params.wav).toMatchObject({
        type: 'wav_ref',
        path: 'kick',
        channels: 1,
        sampleRate: 44100,
        frameCount: 1000,
        bitDepth: 16,
        pitch: 0.0,
        playback: 'one-shot',
    });
    expect(sampler[0].params.wav.loops).toEqual([]);
    expect(sampler[0].params.wav.cuePoints).toEqual([]);
});
```

Update the nested directory test (line 787-791):

```typescript
test('$wavs() traverses nested directories', () => {
    const result = execWithWavs('$sampler($wavs().tables.boom, 5).out()');
    const sampler = findModules(result.patch, '$sampler');
    expect(sampler.length).toBe(1);
    expect(sampler[0].params.wav).toMatchObject({
        type: 'wav_ref',
        path: 'tables/boom',
        channels: 2,
        sampleRate: 44100,
    });
});
```

- [ ] **Step 3: Add a test for metadata with loops and cue points**

Add a new test after the existing ones:

```typescript
test('$wavs() exposes metadata with loops and cue points', () => {
    const metadataLoadWav = (path: string) => ({
        channels: 1,
        frameCount: 44100,
        path,
        sampleRate: 44100,
        duration: 1.0,
        bitDepth: 24,
        pitch: 0.75,
        playback: 'loop' as const,
        bpm: 120.0,
        beats: 4,
        timeSignature: { num: 4, den: 4 },
        loops: [
            { loopType: 'forward', start: 0.0, end: 0.5 },
            { loopType: 'pingpong', start: 0.25, end: 0.75 },
        ],
        cuePoints: [
            { position: 0.0, label: 'Start' },
            { position: 0.5, label: 'Middle' },
        ],
    });

    const result = executePatchScript(
        '$sampler($wavs().kick, 5).out()',
        schemas,
        {
            ...DEFAULT_EXECUTION_OPTIONS,
            wavsFolderTree: wavsFolderTree as any,
            loadWav: metadataLoadWav,
        },
    );
    const sampler = findModules(result.patch, '$sampler');
    const wav = sampler[0].params.wav;

    expect(wav.sampleRate).toBe(44100);
    expect(wav.duration).toBe(1.0);
    expect(wav.bitDepth).toBe(24);
    expect(wav.pitch).toBe(0.75);
    expect(wav.playback).toBe('loop');
    expect(wav.bpm).toBe(120.0);
    expect(wav.beats).toBe(4);
    expect(wav.timeSignature).toEqual({ num: 4, den: 4 });
    expect(wav.loops).toEqual([
        { type: 'forward', start: 0.0, end: 0.5 },
        { type: 'pingpong', start: 0.25, end: 0.75 },
    ]);
    expect(wav.cuePoints).toEqual([
        { position: 0.0, label: 'Start' },
        { position: 0.5, label: 'Middle' },
    ]);
});
```

- [ ] **Step 4: Run tests**

Run: `yarn test:unit`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src/main/dsl/__tests__/executor.test.ts
git commit -m "test(wav-metadata): update DSL tests for metadata fields"
```

---

### Task 9: Regenerate N-API types and final verification

**Files:**

- Regenerate: `crates/modular/index.d.ts`

- [ ] **Step 1: Build native module**

Run: `yarn build-native`
Expected: Builds successfully

- [ ] **Step 2: Regenerate TypeScript types**

Run: `yarn generate-lib`
Expected: Updates `crates/modular/index.d.ts` with new `WavLoadInfo`, `WavLoopInfo`, `WavCuePointInfo`, `WavTimeSignature` types

- [ ] **Step 3: Run all Rust tests**

Run: `cargo test -p modular`
Expected: ALL PASS

- [ ] **Step 4: Run all TypeScript tests**

Run: `yarn test:unit`
Expected: ALL PASS

- [ ] **Step 5: Run TypeScript type check**

Run: `yarn typecheck`
Expected: PASS

- [ ] **Step 6: Commit generated files**

```bash
git add crates/modular/index.d.ts
git commit -m "chore: regenerate N-API type definitions"
```

Plan complete and saved to `docs/superpowers/plans/2026-04-15-wav-metadata.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
