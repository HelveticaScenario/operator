//! Loading WAV files into `$granularSampler`'s buffers, with a cache of
//! recent loads.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::types::WavData;

use super::buffer::{GrainBuffer, MAX_LOADED_FRAMES};
use super::chain::Media;
use super::grain::HEAD_CROSSING_SAFETY_ZONE;
use super::granular::GranularCore;
use super::primitives::Decimator2;
use super::resample::{MonoHistory, Resampler};

/// Shortest loaded buffer, so grains keep clear of the head on both sides.
pub const MIN_LOADED_FRAMES: usize = 4 * (HEAD_CROSSING_SAFETY_ZONE as usize + 1);

/// Most loads the cache keeps, and most samples they hold (256 MB). A load a
/// voice still plays is never dropped.
const CACHED_LOADS: usize = 16;
const CACHED_SAMPLES: usize = 128 << 20;

/// A file loaded for one quality: a buffer per channel, tagged `source`.
struct Load {
    /// `None` for the silence that stands in for a missing file.
    wav: Option<Arc<WavData>>,
    media: Media,
    source: u64,
    channels: Vec<Arc<GrainBuffer>>,
}

impl Load {
    fn samples(&self) -> usize {
        self.channels.iter().map(|b| b.frames()).sum()
    }

    /// Whether any voice still holds one of the load's buffers.
    fn in_use(&self) -> bool {
        self.channels.iter().any(|b| b.shared_elsewhere())
    }
}

/// Drop the oldest loads no voice plays until at most `max_loads` remain
/// and they hold at most `max_samples`. The newest load always stays.
fn evict(cache: &mut Vec<Load>, max_loads: usize, max_samples: usize) {
    let mut total: usize = cache.iter().map(Load::samples).sum();
    let mut i = 0;
    while (cache.len() > max_loads || total > max_samples) && i + 1 < cache.len() {
        if cache[i].in_use() {
            i += 1;
        } else {
            total -= cache.remove(i).samples();
        }
    }
}

/// `wav` loaded for `media` and the id tagging its buffers, reusing a recent
/// load, so a patch update or an engine restart finds its files loaded.
/// Each cached load holds its file, so the file's address identifies it for
/// as long as the load is cached. Main thread only.
pub fn cached_load(wav: Option<&Arc<WavData>>, media: Media) -> (u64, Vec<Arc<GrainBuffer>>) {
    static CACHE: Mutex<Vec<Load>> = Mutex::new(Vec::new());
    // 0 tags a recording.
    static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let found = cache.iter().position(|l| {
        l.media == media
            && match (&l.wav, wav) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    });
    if let Some(i) = found {
        let load = cache.remove(i);
        let result = (load.source, load.channels.clone());
        cache.push(load);
        return result;
    }
    let channels: Vec<_> = match wav {
        Some(w) => (0..w.channel_count().max(1))
            .map(|c| Arc::new(load_buffer(w, c, media)))
            .collect(),
        None => vec![Arc::new(silent_buffer(media))],
    };
    let source = NEXT_SOURCE.fetch_add(1, Ordering::Relaxed);
    cache.push(Load {
        wav: wav.cloned(),
        media,
        source,
        channels: channels.clone(),
    });
    evict(&mut cache, CACHED_LOADS, CACHED_SAMPLES);
    (source, channels)
}

/// Record channel `channel` of `wav` as `media` records its input: converted
/// to the hardware rate, halved for clean tape's grain engine, and stored
/// through the recording path at the quality's bit depth with no feedback.
/// The buffer is as long as the file, within its limits. Main thread only.
fn load_buffer(wav: &WavData, channel: usize, media: Media) -> GrainBuffer {
    let samples = wav.channel(channel);
    let hardware_rate = media.sample_rate();
    let ratio = media.granular_ratio();
    let grain_rate = hardware_rate / ratio as f32;
    let frames = (samples.len() as f64 * grain_rate as f64 / wav.sample_rate() as f64).round();
    let frames = (frames as usize).clamp(MIN_LOADED_FRAMES, MAX_LOADED_FRAMES);
    let mut core = GranularCore::default();
    core.allocate_frames(frames);
    core.reset(grain_rate, media.resolution(), frames);

    let mut resampler = Resampler::default();
    resampler.allocate(wav.sample_rate());
    resampler.configure(wav.sample_rate(), hardware_rate);
    let mut history = MonoHistory::default();
    history.allocate();
    // Hardware samples produced before the file reaches the converter's
    // output.
    let mut lead = (resampler.input_delay() * resampler.ratio()).round() as usize;
    let mut decimator = Decimator2::default();
    let mut pair = 0;
    let mut written = 0;
    let mut n = 0;
    while written < frames {
        history.write(
            resampler.input_index(),
            samples.get(n).copied().unwrap_or(0.0),
        );
        n += 1;
        resampler.tick(|inner, _| {
            if lead > 0 {
                lead -= 1;
                return;
            }
            if written == frames {
                return;
            }
            let x = inner.read(&history);
            if ratio == 1 {
                core.load(x);
                written += 1;
                return;
            }
            decimator.push(x);
            pair += 1;
            if pair == ratio {
                pair = 0;
                core.load(decimator.output());
                written += 1;
            }
        });
    }
    let mut buffer = std::mem::take(&mut core.buffer);
    buffer.share();
    buffer
}

/// The shortest buffer, silent, for a WAV that is missing or empty.
pub fn silent_buffer(media: Media) -> GrainBuffer {
    let mut buffer = GrainBuffer::default();
    buffer.allocate_frames(MIN_LOADED_FRAMES);
    buffer.reset(media.resolution(), MIN_LOADED_FRAMES);
    buffer.share();
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(source: u64, frames: usize) -> Load {
        let mut buffer = GrainBuffer::default();
        buffer.allocate_frames(frames);
        buffer.share();
        Load {
            wav: None,
            media: Media::Digital,
            source,
            channels: vec![Arc::new(buffer)],
        }
    }

    fn sources(cache: &[Load]) -> Vec<u64> {
        cache.iter().map(|l| l.source).collect()
    }

    #[test]
    fn evicts_the_oldest_loads_past_the_count() {
        let mut cache: Vec<_> = (1..=5).map(|s| load(s, 100)).collect();
        evict(&mut cache, 3, usize::MAX);
        assert_eq!(sources(&cache), vec![3, 4, 5]);
    }

    #[test]
    fn evicts_past_the_sample_budget_but_keeps_the_newest() {
        let mut cache: Vec<_> = (1..=3).map(|s| load(s, 1000)).collect();
        evict(&mut cache, 16, 1500);
        assert_eq!(sources(&cache), vec![3]);
        evict(&mut cache, 16, 10);
        assert_eq!(sources(&cache), vec![3]);
    }

    #[test]
    fn keeps_loads_a_voice_still_plays() {
        let mut cache: Vec<_> = (1..=3).map(|s| load(s, 1000)).collect();
        let playing = GrainBuffer::clone(&cache[0].channels[0]);
        evict(&mut cache, 1, usize::MAX);
        assert_eq!(sources(&cache), vec![1, 3]);
        drop(playing);
        evict(&mut cache, 1, usize::MAX);
        assert_eq!(sources(&cache), vec![3]);
    }
}
