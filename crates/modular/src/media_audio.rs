//! Decodes the audio track of a media file (mp4, m4v, mov, mkv, webm with
//! Vorbis, ogg, mp3, flac, wav) into per-channel f32 samples.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Longest audio track that is decoded into memory, in seconds.
pub const MAX_DURATION_SECONDS: u64 = 600;

/// Most channels kept from a track; the engine's ports are narrower than any
/// real-world layout.
const MAX_CHANNELS: usize = 8;

pub struct DecodedAudio {
    pub channels: Vec<Vec<f32>>,
    pub sample_rate: u32,
}

/// Decodes the first audio track of `path`. `Ok(None)` means the file has no
/// audio track. Encoder delay is trimmed when the container records it, so the
/// first sample lines up with the first video frame.
pub fn decode_audio(path: &Path) -> Result<Option<DecodedAudio>, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut format = probed.format;

    let Some(track) = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
    else {
        return Ok(None);
    };
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    if sample_rate == 0 {
        return Ok(None);
    }
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("cannot decode the audio of {}: {e}", path.display()))?;

    let max_frames = MAX_DURATION_SECONDS * u64::from(sample_rate);
    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut scratch: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet is skipped; the rest of the track still plays.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => {
                return Err(format!(
                    "cannot decode the audio of {}: {e}",
                    path.display()
                ));
            }
        };
        let spec = *decoded.spec();
        let count = spec.channels.count().min(MAX_CHANNELS);
        if channels.is_empty() {
            channels = vec![Vec::new(); count];
        }
        let samples =
            scratch.get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, spec));
        if samples.capacity() < decoded.capacity() * spec.channels.count() {
            *samples = SampleBuffer::new(decoded.capacity() as u64, spec);
        }
        samples.copy_interleaved_ref(decoded);
        let stride = spec.channels.count();
        for frame in samples.samples().chunks_exact(stride) {
            for (channel, out) in channels.iter_mut().enumerate() {
                out.push(frame[channel]);
            }
        }
        if channels.first().map_or(0, Vec::len) as u64 > max_frames {
            return Err(format!(
                "the audio of {} is longer than {} minutes",
                path.display(),
                MAX_DURATION_SECONDS / 60
            ));
        }
    }

    if channels.first().is_none_or(Vec::is_empty) {
        return Ok(None);
    }
    Ok(Some(DecodedAudio {
        channels,
        sample_rate,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn decodes_the_audio_track_of_an_mp4() {
        let audio = decode_audio(&fixture("av.mp4")).unwrap().unwrap();
        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.channels.len(), 2);
        // Half a second of audio, give or take the codec's frame granularity.
        let frames = audio.channels[0].len();
        assert!((20_000..=24_500).contains(&frames), "frames = {frames}");
        assert_eq!(audio.channels[1].len(), frames);
    }

    #[test]
    fn decoded_audio_carries_the_tone() {
        let audio = decode_audio(&fixture("av.mp4")).unwrap().unwrap();
        let peak = audio.channels[0]
            .iter()
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(peak > 0.05 && peak <= 0.2, "peak = {peak}");
    }

    #[test]
    fn a_video_without_sound_has_no_audio() {
        assert!(decode_audio(&fixture("silent.mp4")).unwrap().is_none());
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(decode_audio(&fixture("absent.mp4")).is_err());
    }

    #[test]
    fn a_file_that_is_not_media_is_an_error() {
        let error = decode_audio(&fixture("../../Cargo.toml"));
        assert!(error.is_err());
    }
}
