//! FFT-based mipmap generation for anti-aliased wavetable playback.
//!
//! [`PreparedWavetable`] takes a [`WavData`], splits it into equal-size frames,
//! and generates a band-limited mipmap pyramid — each level halves the spectral
//! bandwidth of the one before it. At read time, the oscillator picks the
//! coarsest level whose cutoff is still above its playback frequency, so higher
//! pitches use lower-harmonic-content versions of the wave and avoid aliasing.
//!
//! Construction is main-thread only and allocates freely. [`read_sample`] and
//! [`mipmap_level_for_freq`] are allocation-free and suitable for the audio
//! thread.
//!
//! [`read_sample`]: PreparedWavetable::read_sample
//! [`mipmap_level_for_freq`]: PreparedWavetable::mipmap_level_for_freq

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::types::WavData;

/// Fallback frame size used when the WAV has no detected frame size or a
/// degenerate value (0 or 1).
const DEFAULT_FRAME_SIZE: usize = 2048;

/// Oversampling factor for mipmap tables. The IFFT output is computed at
/// `frame_size * OVERSAMPLE` samples per frame, giving the linear interpolation
/// in `read_sample` much more accuracy without needing higher-order filters.
const OVERSAMPLE: usize = 8;

/// A WAV loaded and prepared for band-limited wavetable playback.
///
/// Stores a mipmap pyramid of time-domain samples, one level per spectral
/// halving. All levels share the same frame count and table size; within a
/// level, frames are concatenated so indexing is
/// `levels[level][frame_idx * table_size + sample_idx]`.
#[derive(Debug, Clone, Connect)]
pub struct PreparedWavetable {
    /// Mipmap levels. Each level is `frame_count * table_size` samples,
    /// frames stored back-to-back.
    pub levels: Vec<Vec<f32>>,
    /// Original samples per frame from the WAV metadata.
    pub frame_size: usize,
    /// Samples per frame in the stored tables (`frame_size * OVERSAMPLE`).
    pub table_size: usize,
    /// Number of frames in the table.
    pub frame_count: usize,
    /// Number of mipmap levels. Level 0 is full bandwidth, level
    /// `mipmap_count - 1` has the narrowest bandwidth.
    pub mipmap_count: usize,
    /// Fundamental frequency of the stored wave when played at the target
    /// sample rate (one frame per `frame_size` samples).
    pub base_frequency: f32,
}

impl PreparedWavetable {
    /// Prepare a wavetable from a loaded WAV.
    ///
    /// Reads channel 0. The frame size is taken from `wav.detected_frame_size`,
    /// falling back to [`DEFAULT_FRAME_SIZE`] when the metadata is absent or
    /// degenerate. Frames past the end of the channel (i.e. if the sample
    /// count isn't a clean multiple of `frame_size`) are discarded.
    ///
    /// Main thread only — allocates and runs FFTs.
    pub fn from_wav_data(wav: &WavData, sample_rate: f32) -> Self {
        let mut frame_size = wav.detected_frame_size.unwrap_or(DEFAULT_FRAME_SIZE);
        if frame_size < 2 {
            frame_size = DEFAULT_FRAME_SIZE;
        }

        let total_samples = wav.frame_count();
        let frame_count = total_samples / frame_size;
        let table_size = frame_size * OVERSAMPLE;
        let base_frequency = sample_rate / frame_size as f32;

        // Integer log2(frame_size), at least 1 level.
        let mipmap_count = integer_log2(frame_size).max(1);

        let channel = 0;

        // Pre-allocate one Vec<f32> per level, all zero-initialized.
        let level_len = frame_count * table_size;
        let mut levels: Vec<Vec<f32>> =
            (0..mipmap_count).map(|_| vec![0.0f32; level_len]).collect();

        if total_samples == 0 || wav.channel_count() == 0 || frame_count == 0 {
            return Self {
                levels,
                frame_size,
                table_size,
                frame_count,
                mipmap_count,
                base_frequency,
            };
        }

        let mut planner = FftPlanner::<f32>::new();
        let fft_forward = planner.plan_fft_forward(frame_size);
        let fft_inverse = planner.plan_fft_inverse(table_size);

        // Scratch buffers reused across frames.
        let mut spectrum: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); frame_size];
        let mut padded: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); table_size];
        // The forward FFT (size frame_size) leaves bins scaled by frame_size.
        // The inverse FFT (size table_size) is unnormalized in rustfft.
        // To recover the original amplitude we divide by frame_size.
        let inv_n = 1.0 / frame_size as f32;

        for frame_idx in 0..frame_count {
            // Load channel 0 samples for this frame into the FFT input buffer.
            let src_base = frame_idx * frame_size;
            for i in 0..frame_size {
                let s = wav.read(channel, src_base + i);
                spectrum[i] = Complex::new(s, 0.0);
            }

            fft_forward.process(&mut spectrum);

            // Level 0 = full bandwidth. For each subsequent level, zero bins
            // at index >= frame_size / (2 << l). Mirror to the negative-
            // frequency half so the IFFT produces real output.
            //
            // The forward FFT produces `frame_size` bins. We zero-pad the
            // spectrum to `table_size` bins for the inverse FFT, placing the
            // positive-frequency half at the start and the negative-frequency
            // half at the end, with zeros in the middle. This is the standard
            // zero-padded IFFT oversampling approach.
            for level in 0..mipmap_count {
                // Clear the padded buffer.
                for c in padded.iter_mut() {
                    *c = Complex::new(0.0, 0.0);
                }

                let half = frame_size / 2;

                // Determine how many bins to keep for this level.
                let cutoff = if level == 0 {
                    half // full bandwidth: keep bins 0..half
                } else {
                    frame_size / (2usize << level)
                };

                // Copy positive-frequency bins [0..cutoff) to padded[0..cutoff).
                for k in 0..cutoff {
                    padded[k] = spectrum[k];
                }

                // Copy negative-frequency bins. In the original frame_size FFT,
                // negative frequencies are at indices [frame_size-cutoff+1 .. frame_size-1].
                // In the padded table_size buffer, they go at
                // [table_size-cutoff+1 .. table_size-1].
                for k in 1..cutoff {
                    padded[table_size - k] = spectrum[frame_size - k];
                }

                // DC bin (index 0) is already copied. Nyquist bin (index half)
                // is excluded for all levels since cutoff <= half.

                fft_inverse.process(&mut padded);

                let dest_base = frame_idx * table_size;
                let dest = &mut levels[level][dest_base..dest_base + table_size];
                for (i, c) in padded.iter().enumerate() {
                    dest[i] = c.re * inv_n;
                }
            }
        }

        Self {
            levels,
            frame_size,
            table_size,
            frame_count,
            mipmap_count,
            base_frequency,
        }
    }

    /// Read an interpolated sample from the prepared wavetable.
    ///
    /// - `level` is clamped to `[0, mipmap_count - 1]`.
    /// - `phase` is wrapped modulo 1.0 and linearly interpolated within a
    ///   frame.
    /// - `frame` is clamped to `[0, frame_count - 1]` and linearly
    ///   crossfaded between adjacent frames.
    ///
    /// Allocation-free; safe to call from the audio thread.
    #[inline]
    pub fn read_sample(&self, level: usize, frame: f32, phase: f32) -> f32 {
        if self.frame_count == 0 || self.table_size == 0 {
            return 0.0;
        }

        let level = level.min(self.mipmap_count.saturating_sub(1));
        let buf = &self.levels[level];

        // Frame selection — clamp to [0, frame_count - 1].
        let max_frame_f = (self.frame_count - 1) as f32;
        let frame = frame.clamp(0.0, max_frame_f);
        let f0 = frame.floor();
        let f0_idx = f0 as usize;
        let f1_idx = (f0_idx + 1).min(self.frame_count - 1);
        let frame_frac = frame - f0;

        // Phase wrap to [0, 1).
        let phase = phase - phase.floor();
        let idx = phase * self.table_size as f32;
        let i0 = idx as usize;
        let i0 = if i0 >= self.table_size { 0 } else { i0 };
        let i1 = if i0 + 1 >= self.table_size { 0 } else { i0 + 1 };
        let phase_frac = idx - (i0 as f32);

        let base0 = f0_idx * self.table_size;
        let base1 = f1_idx * self.table_size;

        let s0a = buf[base0 + i0];
        let s0b = buf[base0 + i1];
        let s0 = s0a + (s0b - s0a) * phase_frac;

        let s1a = buf[base1 + i0];
        let s1b = buf[base1 + i1];
        let s1 = s1a + (s1b - s1a) * phase_frac;

        s0 + (s1 - s0) * frame_frac
    }

    /// Choose a mipmap level for a given playback frequency.
    ///
    /// Each level halves the spectral bandwidth; level `l` is appropriate
    /// when the playback frequency is `base_frequency * 2^l` or higher. The
    /// result is clamped to `[0, mipmap_count - 1]`.
    ///
    /// Allocation-free; safe to call from the audio thread.
    #[inline]
    pub fn mipmap_level_for_freq(&self, freq: f32) -> usize {
        if self.mipmap_count == 0 {
            return 0;
        }
        if self.base_frequency <= 0.0 || freq <= self.base_frequency {
            return 0;
        }
        let ratio = freq / self.base_frequency;
        let level = ratio.log2().max(0.0) as usize;
        level.min(self.mipmap_count - 1)
    }
}

/// Floor(log2(n)) for n >= 1. Returns 0 for n == 0.
#[inline]
fn integer_log2(n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    (usize::BITS - 1 - n.leading_zeros()) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SampleBuffer;
    use std::f32::consts::TAU;

    fn make_sine_frame(size: usize, phase_offset: f32) -> Vec<f32> {
        (0..size)
            .map(|i| (TAU * (i as f32 / size as f32) + phase_offset).sin())
            .collect()
    }

    fn make_cosine_frame(size: usize) -> Vec<f32> {
        (0..size)
            .map(|i| (TAU * (i as f32 / size as f32)).cos())
            .collect()
    }

    #[test]
    fn single_sine_frame_prepares_correctly() {
        let size = 2048;
        let samples = vec![make_sine_frame(size, 0.0)];
        let wav = WavData::new(SampleBuffer::from_samples(samples, 48000.0), Some(size));
        let prep = PreparedWavetable::from_wav_data(&wav, 48000.0);

        assert_eq!(prep.frame_count, 1);
        assert_eq!(prep.frame_size, size);
        assert!(
            prep.mipmap_count >= 10,
            "expected >=10 mipmap levels, got {}",
            prep.mipmap_count
        );
        // log2(2048) = 11.
        assert_eq!(prep.mipmap_count, 11);

        // Sine starts at 0.
        let s0 = prep.read_sample(0, 0.0, 0.0);
        assert!(s0.abs() < 1e-4, "expected ~0 at phase 0, got {s0}");

        // Quarter-cycle of sine is +1.
        let s_quarter = prep.read_sample(0, 0.0, 0.25);
        assert!(
            (s_quarter - 1.0).abs() < 1e-3,
            "expected ~1.0 at phase 0.25, got {s_quarter}"
        );

        // Sine fundamental is bin 1, far below any mipmap cutoff, so level 1
        // must be nearly identical to level 0.
        let s0_l1 = prep.read_sample(1, 0.0, 0.0);
        let s_quarter_l1 = prep.read_sample(1, 0.0, 0.25);
        assert!(s0_l1.abs() < 1e-3, "level1 phase 0: {s0_l1}");
        assert!(
            (s_quarter_l1 - 1.0).abs() < 1e-3,
            "level1 phase 0.25: {s_quarter_l1}"
        );
    }

    #[test]
    fn multi_frame_crossfades_between_frames() {
        let size = 2048;
        let mut ch: Vec<f32> = Vec::with_capacity(size * 2);
        ch.extend(make_sine_frame(size, 0.0));
        ch.extend(make_cosine_frame(size));
        let wav = WavData::new(SampleBuffer::from_samples(vec![ch], 48000.0), Some(size));
        let prep = PreparedWavetable::from_wav_data(&wav, 48000.0);

        assert_eq!(prep.frame_count, 2);

        // At phase 0.25: sine = +1, cosine = 0. Midpoint ~= 0.5.
        let mid = prep.read_sample(0, 0.5, 0.25);
        assert!((mid - 0.5).abs() < 5e-3, "expected ~0.5 midway, got {mid}");

        // At frame=0, phase=0.25 should still be ~1.0 (sine).
        let f0 = prep.read_sample(0, 0.0, 0.25);
        assert!((f0 - 1.0).abs() < 1e-3, "frame 0 phase 0.25: {f0}");

        // At frame=1, phase=0.0 should be ~1.0 (cosine at 0).
        let f1 = prep.read_sample(0, 1.0, 0.0);
        assert!((f1 - 1.0).abs() < 1e-3, "frame 1 phase 0: {f1}");
    }

    #[test]
    fn mipmap_attenuates_high_frequency_content() {
        let size = 2048;
        // Put a cosine at bin (size/2 - 2) — high, above the level-1 cutoff
        // of size/4 = 512.
        let bin = size / 2 - 2;
        let freq_norm = bin as f32 / size as f32;
        let ch: Vec<f32> = (0..size)
            .map(|i| (TAU * freq_norm * i as f32).cos())
            .collect();
        let wav = WavData::new(SampleBuffer::from_samples(vec![ch], 48000.0), Some(size));
        let prep = PreparedWavetable::from_wav_data(&wav, 48000.0);

        let rms = |level: usize| -> f32 {
            let buf = &prep.levels[level];
            let sum_sq: f32 = buf.iter().map(|x| x * x).sum();
            (sum_sq / buf.len() as f32).sqrt()
        };

        let rms0 = rms(0);
        let rms1 = rms(1);
        assert!(rms0 > 0.1, "level 0 RMS too low: {rms0}");
        assert!(
            rms1 < rms0 * 0.01,
            "expected level 1 to zero out the high bin; rms0={rms0} rms1={rms1}"
        );
    }

    #[test]
    fn mipmap_level_for_freq_picks_correct_level() {
        let size = 2048;
        let ch = make_sine_frame(size, 0.0);
        let wav = WavData::new(SampleBuffer::from_samples(vec![ch], 48000.0), Some(size));
        let prep = PreparedWavetable::from_wav_data(&wav, 48000.0);

        // base_frequency = 48000 / 2048 ≈ 23.4375 Hz.
        let base = prep.base_frequency;
        assert!((base - 48000.0 / 2048.0).abs() < 1e-3);

        assert_eq!(prep.mipmap_level_for_freq(base), 0);
        assert_eq!(prep.mipmap_level_for_freq(base * 0.5), 0);
        assert_eq!(prep.mipmap_level_for_freq(base * 2.0), 1);
        // log2(100/23.4375) ≈ 2.09 → 2.
        assert_eq!(prep.mipmap_level_for_freq(100.0), 2);
        // log2(200/23.4375) ≈ 3.09 → 3.
        assert_eq!(prep.mipmap_level_for_freq(200.0), 3);
        // log2(10000/23.4375) ≈ 8.74 → 8 (unclamped).
        assert_eq!(prep.mipmap_level_for_freq(10000.0), 8);
        // Clamped at the top: base * 2^10 ≈ 24000 Hz; anything well above
        // that saturates.
        assert_eq!(prep.mipmap_level_for_freq(96000.0), prep.mipmap_count - 1);
    }

    #[test]
    fn integer_log2_matches_expected() {
        assert_eq!(integer_log2(0), 0);
        assert_eq!(integer_log2(1), 0);
        assert_eq!(integer_log2(2), 1);
        assert_eq!(integer_log2(3), 1);
        assert_eq!(integer_log2(4), 2);
        assert_eq!(integer_log2(2048), 11);
        assert_eq!(integer_log2(4096), 12);
    }

    #[test]
    fn fallback_frame_size_when_metadata_missing() {
        let size = 2048;
        let ch = make_sine_frame(size, 0.0);
        let wav = WavData::new(SampleBuffer::from_samples(vec![ch], 48000.0), None);
        let prep = PreparedWavetable::from_wav_data(&wav, 48000.0);
        assert_eq!(prep.frame_size, DEFAULT_FRAME_SIZE);
    }

    #[test]
    fn sub_frame_size_wav_produces_silent_table() {
        // Wav with fewer samples than fallback frame_size — should not panic,
        // should be silent.
        let samples = vec![vec![0.5f32; 100]]; // single channel, 100 samples, frame_size=2048 fallback
        let wav = WavData::new(SampleBuffer::from_samples(samples, 48000.0), None);
        let prep = PreparedWavetable::from_wav_data(&wav, 48000.0);
        assert_eq!(prep.frame_count, 0);
        // read_sample should return silence without panicking.
        assert_eq!(prep.read_sample(0, 0.0, 0.0), 0.0);
        assert_eq!(prep.read_sample(5, 0.5, 0.5), 0.0);
    }
}
