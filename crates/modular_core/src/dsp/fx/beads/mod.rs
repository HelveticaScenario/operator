//! Port of the granular mode of Mutable Instruments Beads
//! (beads/dsp, copyright 2017 Emilie Gillet, MIT licence), split into
//! modules: grain voices (`$granular`, `$granularSampler`), wow and flutter
//! (`$wowFlutter`) and the reverb with its limiters (`$reverb`). Grains start
//! on an external trigger.

mod buffer;
mod chain;
mod flutter;
mod grain;
mod grains;
mod grains_chain;
mod granular;
mod player;
mod primitives;
mod queue;
mod resample;
mod reverb;
mod sampler;
mod verb;
mod voice;
mod wow_flutter;

#[cfg(test)]
mod tests;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::{PolySignal, PolySignalExt};

use chain::Media;
pub use flutter::WowFlutter;
pub use grains::Granular;
use player::GrainParams;
use primitives::Frame;
pub use sampler::GranularSampler;
pub use verb::BeadsReverb;

/// Volts per hardware full scale.
const FULL_SCALE_VOLTS: f32 = 5.0;

/// Sound character, after Beads' four quality settings. Lower qualities
/// sound darker and grittier and hold more audio.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum GranularQuality {
    /// Clean, with 4 s of audio.
    #[default]
    Digital,
    /// Slightly gritty early digital, with 8 s of audio.
    Vintage,
    /// Darker, softer grains, with 10.7 s of audio.
    CleanTape,
    /// Lo-fi and noisy, with 16 s of audio.
    DirtyTape,
}

impl GranularQuality {
    fn media(self) -> Media {
        match self {
            GranularQuality::Digital => Media::Digital,
            GranularQuality::Vintage => Media::VintageDigital,
            GranularQuality::CleanTape => Media::CleanTape,
            GranularQuality::DirtyTape => Media::DirtyTape,
        }
    }
}

/// Map a 0–5 V control to the hardware's 0–1 range.
#[inline]
fn unit(v: f32) -> f32 {
    (v / FULL_SCALE_VOLTS).clamp(0.0, 1.0)
}

#[inline]
fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Unspread grain controls of `voice`, read from the grain inputs in volts.
fn grain_params(
    time: &Option<PolySignal>,
    size: &Option<PolySignal>,
    shape: &Option<PolySignal>,
    freq: &Option<PolySignal>,
    root_freq: &Option<PolySignal>,
    voice: usize,
) -> GrainParams {
    let freq = freq.value_or(voice, 0.0);
    let root = root_freq.value_or(voice, 0.0);
    GrainParams {
        time: unit(time.value_or(voice, 0.0)),
        size: (finite(size.value_or(voice, 2.75)) / FULL_SCALE_VOLTS).clamp(-1.0, 1.0),
        shape: unit(shape.value_or(voice, 3.33)),
        pitch: finite((freq - root) * 12.0).clamp(-84.0, 84.0),
        spread: 0.0,
    }
}

/// Channel `ch` of an audio input in hardware units.
#[inline]
fn input_value(input: &PolySignal, ch: usize) -> f32 {
    finite(input.get_value(ch)) * (1.0 / FULL_SCALE_VOLTS)
}

/// Fold a poly input to stereo in hardware units: even channels feed the left
/// side and odd channels the right; with an odd channel count the last
/// channel feeds both.
fn fold_input(input: &PolySignal) -> Frame {
    let channels = input.channels();
    let mut l = 0.0;
    let mut r = 0.0;
    for ch in 0..channels {
        let v = input.get_value(ch);
        if ch % 2 == 0 {
            l += v;
        } else {
            r += v;
        }
    }
    if channels % 2 == 1 {
        r += input.get_value(channels - 1);
    }
    Frame::new(finite(l), finite(r)) * (1.0 / FULL_SCALE_VOLTS)
}
