//! Onset fade-in primitive.

/// Quick fade time, in seconds, for de-clicking a module's onset. Short enough
/// to be inaudible as a fade, long enough to remove the click.
pub const DEFAULT_FADE_IN_SECS: f32 = 0.005;

/// One-shot fade-in: ramps a gain from 0 to 1 over a fixed time, then holds at
/// 1. Multiply a module's output by [`FadeIn::advance`] to suppress the click
/// from the signal stepping out of silence when the module is first added.
///
/// The default state (`gain == 0.0`) is a freshly added module, so it fades in.
/// A channel whose state is carried over by `transfer_state_from` on a patch
/// edit arrives with `gain` already at 1.0 and keeps playing without re-fading.
#[derive(Clone, Copy, Debug, Default)]
pub struct FadeIn {
    /// Current gain in [0, 1]. 0 = not yet faded in, 1 = complete.
    pub gain: f32,
}

impl FadeIn {
    /// Per-sample ramp increment for a `secs`-long fade at `sample_rate`.
    /// Sample-rate-only, so compute it once in `init` and pass to
    /// [`Self::advance`].
    #[inline]
    pub fn increment(secs: f32, sample_rate: f32) -> f32 {
        1.0 / (secs * sample_rate).max(1.0)
    }

    /// Advance the ramp by one sample and return the current gain.
    #[inline]
    pub fn advance(&mut self, increment: f32) -> f32 {
        self.gain = (self.gain + increment).min(1.0);
        self.gain
    }
}
