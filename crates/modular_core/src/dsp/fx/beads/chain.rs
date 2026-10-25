//! Pieces shared across the Beads modules: the hardware's media settings,
//! its control-block clock and its stereo placement of voices.

use super::buffer::{Resolution, mulaw_round_trip};

/// Control blocks per second; parameters are smoothed and ramped per block.
const BLOCK_RATE: f32 = 4000.0;
/// Per-block one-pole coefficient the hardware applies to its pots.
pub const POT_LP: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Media {
    #[default]
    Digital,
    VintageDigital,
    CleanTape,
    DirtyTape,
}

impl Media {
    /// Rate the hardware codec runs at.
    pub fn sample_rate(self) -> f32 {
        match self {
            Media::Digital | Media::CleanTape => 48000.0,
            Media::VintageDigital => 32000.0,
            Media::DirtyTape => 24000.0,
        }
    }

    /// Codec samples per grain-engine sample.
    pub fn granular_ratio(self) -> usize {
        match self {
            Media::CleanTape => 2,
            _ => 1,
        }
    }

    pub fn resolution(self) -> Resolution {
        match self {
            Media::Digital => Resolution::Linear16,
            Media::VintageDigital | Media::CleanTape => Resolution::Linear12,
            Media::DirtyTape => Resolution::MuLaw8,
        }
    }

    /// Buffer length in grain-engine samples, as on the hardware: 4 s at
    /// 48 kHz, 8 s at 32 kHz, 10.7 s at 24 kHz (clean tape) and 16 s at
    /// 24 kHz (dirty tape).
    pub fn max_delay(self) -> usize {
        match self {
            Media::Digital => 192_000,
            Media::VintageDigital | Media::CleanTape => 256_000,
            Media::DirtyTape => 384_000,
        }
    }

    /// The dry signal as the codec path degrades it.
    #[inline]
    pub fn colour(self, x: f32) -> f32 {
        match self {
            Media::Digital | Media::CleanTape => x,
            Media::VintageDigital => (x * 4096.0) as i32 as f32 / 4096.0,
            Media::DirtyTape => mulaw_round_trip(x),
        }
    }
}

/// Left and right gains of voice `i` of `n`, panned equal-power from
/// −`width` to +`width` volts across the voices and scaled so a centred voice
/// keeps unity gain. A single voice stays centred.
pub fn voice_place(i: usize, n: usize, width: f32) -> [f32; 2] {
    let pan = if n > 1 {
        let spacing = 2.0 * width / (n - 1) as f32;
        (-width + i as f32 * spacing).clamp(-5.0, 5.0)
    } else {
        0.0
    };
    let p = (pan + 5.0) / 10.0;
    [(2.0 * (1.0 - p)).sqrt(), (2.0 * p).sqrt()]
}

/// Clock of the control blocks at the rate its owner runs at.
#[derive(Default)]
pub struct BlockClock {
    size: usize,
    pos: usize,
}

impl BlockClock {
    pub fn configure(&mut self, sample_rate: f32) {
        self.size = (sample_rate / BLOCK_RATE) as usize;
        self.pos = 0;
    }

    /// Samples per block.
    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Advance by one sample; true when a block starts at this sample.
    #[inline]
    pub fn tick(&mut self) -> bool {
        let starts = self.pos == 0;
        self.pos = (self.pos + 1) % self.size;
        starts
    }
}
