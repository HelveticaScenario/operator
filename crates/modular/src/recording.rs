//! Off-audio-thread WAV recording.
//!
//! The audio callback must never touch the filesystem, so a recording session
//! is split across a pre-allocated lock-free ring buffer: the callback pushes
//! f32 samples into the ring (dropping and counting them when it is full),
//! and a dedicated writer thread drains the ring and performs all hound
//! encoding and disk I/O. The WAV file declares 32-bit float samples, matching
//! what the callback enqueues regardless of the cpal stream's sample type.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use hound::{WavSpec, WavWriter};
use rtrb::{Consumer, Producer, RingBuffer};

/// One second of mono samples at the stream rate (rounded up to a power of
/// two), so a disk stall must exceed a full second before samples drop.
fn ring_capacity(sample_rate: u32) -> usize {
    (sample_rate as usize).next_power_of_two()
}

/// How long the writer thread sleeps when the ring is empty.
const WRITER_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Audio-thread half of a recording session. `push` is wait-free and
/// allocation-free; overflow is counted here and reported to stderr by the
/// writer thread, never from the callback.
pub struct RecordingFeed {
    producer: Producer<f32>,
    dropped: Arc<AtomicU64>,
}

impl RecordingFeed {
    /// Push one sample into the ring; on a full ring the sample is dropped
    /// and counted.
    #[inline]
    pub fn push(&mut self, sample: f32) {
        if self.producer.push(sample).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Main-thread handle to a recording session's writer thread.
pub struct RecordingSession {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Result<(), hound::Error>>,
}

impl RecordingSession {
    /// Stop the writer thread, draining every sample still in the ring to disk
    /// and finalizing the WAV header. The caller must drop the session's
    /// [`RecordingFeed`] first so the final drain observes every pushed sample.
    pub fn finish(self) -> Result<PathBuf, hound::Error> {
        self.stop.store(true, Ordering::Release);
        match self.handle.join() {
            Ok(Ok(())) => Ok(self.path),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(hound::Error::IoError(std::io::Error::other(
                "recording writer thread panicked",
            ))),
        }
    }
}

/// Create the WAV file at `path` and spawn the writer thread. Returns the
/// audio-thread feed and the main-thread session handle.
pub fn start(
    path: PathBuf,
    sample_rate: u32,
) -> Result<(RecordingFeed, RecordingSession), hound::Error> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let writer = WavWriter::create(&path, spec)?;
    let (producer, consumer) = RingBuffer::new(ring_capacity(sample_rate));
    let dropped = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let handle = std::thread::Builder::new()
        .name("wav-recording-writer".into())
        .spawn({
            let dropped = Arc::clone(&dropped);
            let stop = Arc::clone(&stop);
            move || write_loop(consumer, writer, &stop, &dropped)
        })
        .map_err(hound::Error::IoError)?;
    Ok((
        RecordingFeed { producer, dropped },
        RecordingSession { path, stop, handle },
    ))
}

/// Drain the ring to disk until stopped. The stop flag is read before each
/// drain pass, so the pass that observes it still empties the ring — nothing
/// pushed before `finish` is lost.
fn write_loop(
    mut consumer: Consumer<f32>,
    mut writer: WavWriter<BufWriter<File>>,
    stop: &AtomicBool,
    dropped: &AtomicU64,
) -> Result<(), hound::Error> {
    let mut reported: u64 = 0;
    loop {
        let stopping = stop.load(Ordering::Acquire);
        let mut wrote_any = false;
        while let Ok(sample) = consumer.pop() {
            writer.write_sample(sample)?;
            wrote_any = true;
        }
        let total = dropped.load(Ordering::Relaxed);
        if total > reported {
            eprintln!(
                "[recording] ring buffer full — {} samples dropped so far",
                total
            );
            reported = total;
        }
        if stopping {
            break;
        }
        if !wrote_any {
            std::thread::sleep(WRITER_POLL_INTERVAL);
        }
    }
    writer.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_wav_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "operator_recording_test_{tag}_{}.wav",
            std::process::id()
        ))
    }

    #[test]
    fn finished_recording_reads_back_as_the_pushed_f32_samples() {
        // The file header declares Float32 and the feed carries f32, so every
        // pushed sample must round-trip exactly through a float WAV reader.
        let path = temp_wav_path("roundtrip");
        let (mut feed, session) = start(path.clone(), 48_000).expect("start recording");
        let samples: Vec<f32> = (0..10_000).map(|i| (i as f32) * 1e-4 - 0.5).collect();
        for &s in &samples {
            feed.push(s);
        }
        drop(feed);
        let finished = session.finish().expect("finalize recording");
        assert_eq!(finished, path);

        let mut reader = hound::WavReader::open(&path).expect("open recording");
        let spec = reader.spec();
        assert_eq!(spec.sample_format, hound::SampleFormat::Float);
        assert_eq!(spec.bits_per_sample, 32);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 48_000);
        let read: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(read, samples);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn full_ring_drops_samples_and_counts_them() {
        // With no consumer draining, pushes beyond the ring capacity must be
        // dropped and counted rather than blocking or allocating.
        let (producer, consumer) = RingBuffer::new(4);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut feed = RecordingFeed {
            producer,
            dropped: Arc::clone(&dropped),
        };
        for i in 0..7 {
            feed.push(i as f32);
        }
        assert_eq!(dropped.load(Ordering::Relaxed), 3);
        drop(consumer);
    }
}
