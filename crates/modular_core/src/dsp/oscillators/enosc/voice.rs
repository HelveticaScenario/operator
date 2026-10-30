//! One `$ensemble` voice: a pair of sine oscillators on adjacent scale notes,
//! crossfaded by where the voice's root sits between them.
//!
//! Adapted from the 4ms Ensemble Oscillator. Copyright 4ms Company. Used under
//! GPL v3.

use deserr::Deserr;
use schemars::JsonSchema;
use serde::Serialize;

use crate::dsp::fx::enosc_tables::{
    aa_cheby, aa_fold, aa_segment, fold_amount, interpolate_cheby, interpolate_segment, lookup_fold,
};
use crate::dsp::phase_warp::{PhaseWarp, PhaseWarpState, ResolvedWarp};
use crate::dsp::utils::fade::FadeIn;
use crate::dsp::utils::{sin_turns, voct_to_hz};

use super::scale::{PitchPair, ScaleNotes};

/// Which waveshaper **waveDist** drives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, Serialize, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum WaveDistMode {
    /// Wavefolding.
    #[default]
    Fold,
    /// Chebyshev polynomials, adding harmonics in turn.
    Cheby,
    /// Triangle segments.
    Segment,
}

/// The waveshaper applied to every oscillator's sine, with its amount as a
/// 0–1 knob. At 0 it is the identity and is skipped.
#[derive(Clone, Copy)]
pub(super) struct WaveDist {
    mode: WaveDistMode,
    amount: f32,
}

impl WaveDist {
    pub fn new(mode: WaveDistMode, amount: f32) -> Self {
        Self { mode, amount }
    }

    pub fn active(&self) -> bool {
        self.amount > 0.0
    }

    // Kept out of line: inlined, the table lookups bloat the oscillator loop
    // and slow it down even while it is off.
    #[inline(never)]
    fn apply(&self, x: f32, freq_norm: f32) -> f32 {
        match self.mode {
            WaveDistMode::Fold => lookup_fold(x, aa_fold(freq_norm, fold_amount(self.amount))),
            WaveDistMode::Cheby => interpolate_cheby(x, aa_cheby(freq_norm, self.amount)),
            WaveDistMode::Segment => interpolate_segment(x, aa_segment(freq_norm, self.amount)),
        }
    }
}

/// Samples per control block: pitch and crossfade update once per block.
pub(super) const CONTROL_BLOCK: usize = 8;

/// Settings shared by every voice of a channel for one control block.
pub(super) struct ControlContext {
    pub inv_sample_rate: f32,
    /// How much of each interval is a hard step rather than a crossfade, 0–0.5.
    pub crossfade_factor: f32,
    /// Per-block coefficient of the crossfade lowpass.
    pub crossfade_coef: f32,
}

/// Settings shared by every voice of a channel for one sample.
pub(super) struct VoiceContext<'a> {
    /// The phase warp with its stage values for this channel.
    pub phase: Option<(&'a PhaseWarp, ResolvedWarp)>,
    pub wave_dist: WaveDist,
    /// Cross-FM depth, before per-oscillator antialiasing.
    pub cross_fm: f32,
    pub fade_inc: f32,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Oscillator {
    pub phase: f32,
    warp: PhaseWarpState,
    /// The V/Oct pitch `freq_norm` was computed from; `NaN` forces a
    /// recomputation.
    pitch: f32,
    freq_norm: f32,
    /// Antialiasing gains for `freq_norm`: the level fade towards Nyquist and
    /// the cross-FM depth rolloff.
    nyquist_fade: f32,
    fm_rolloff: f32,
}

#[derive(Clone, Copy)]
pub(super) struct Voice {
    pub osc: [Oscillator; 2],
    /// Lowpassed crossfade at the end of the current block; `NaN` until the
    /// voice first sounds, so it starts on its target.
    crossfade: f32,
    /// This sample's crossfade, ramping linearly to `crossfade` by
    /// `crossfade_step` per sample over the block.
    crossfade_now: f32,
    crossfade_step: f32,
    pub fade: FadeIn,
    /// The scale lookup for `pair_root` in scale generation `pair_generation`;
    /// a `NaN` root forces a fresh lookup.
    pair: PitchPair,
    pair_root: f32,
    pair_generation: u64,
}

impl Default for Voice {
    fn default() -> Self {
        let osc = Oscillator {
            pitch: f32::NAN,
            ..Default::default()
        };
        Self {
            osc: [osc; 2],
            crossfade: f32::NAN,
            crossfade_now: 0.0,
            crossfade_step: 0.0,
            fade: FadeIn::default(),
            pair: PitchPair {
                p1: 0.0,
                p2: 0.0,
                crossfade: 0.0,
            },
            pair_root: f32::NAN,
            pair_generation: 0,
        }
    }
}

impl Voice {
    /// The scale notes around `root`, looked up only when it or the scale
    /// changed.
    #[inline]
    pub fn pitch_pair(&mut self, scale: &ScaleNotes, root: f32) -> PitchPair {
        if root != self.pair_root || scale.generation != self.pair_generation {
            self.pair = scale.pitch_pair(root);
            self.pair_root = root;
            self.pair_generation = scale.generation;
        }
        self.pair
    }

    /// Restart the voice silent, as when it joins the ensemble.
    pub fn restart(&mut self) {
        self.crossfade = f32::NAN;
        self.fade = FadeIn::default();
    }

    /// Start a control block. `pitches` are the pair's V/Oct pitches and
    /// `crossfade` the scale position between them.
    pub fn control(&mut self, pitches: [f32; 2], crossfade: f32, ctx: &ControlContext) {
        // Shape the crossfade so that notes are easier to find: the middle
        // `1 − 2·factor` of each interval fades, the rest holds a note.
        let width = 1.0 - 2.0 * ctx.crossfade_factor;
        let target = if width > 1e-6 {
            ((crossfade - ctx.crossfade_factor) / width).clamp(0.0, 1.0)
        } else if crossfade < 0.5 {
            0.0
        } else {
            1.0
        };
        let (from, to) = if self.crossfade.is_nan() {
            (target, target)
        } else {
            let next = self.crossfade + (target - self.crossfade) * ctx.crossfade_coef;
            let next = if (target - next).abs() < 1e-6 {
                target
            } else {
                next
            };
            (self.crossfade, next)
        };
        self.crossfade = to;
        self.crossfade_now = from;
        self.crossfade_step = (to - from) / CONTROL_BLOCK as f32;

        for (osc, pitch) in self.osc.iter_mut().zip(pitches) {
            if pitch != osc.pitch {
                let f = voct_to_hz(pitch) * ctx.inv_sample_rate;
                let rolloff = (1.0 - 2.0 * f).max(0.0);
                osc.pitch = pitch;
                osc.freq_norm = f;
                osc.nyquist_fade = (1.0 - 4.0 * f * f).max(0.0);
                osc.fm_rolloff = rolloff * rolloff;
            }
        }
    }

    /// Advance one sample. `mod_in` is the incoming cross-FM phase offset.
    /// Returns the voice's sample in `[-1, 1]` and its cross-FM output.
    #[inline]
    pub fn process(&mut self, mod_in: f32, ctx: &VoiceContext) -> (f32, f32) {
        self.crossfade_now += self.crossfade_step;
        let cf = self.crossfade_now;

        // The silent oscillator follows the sounding one, so it fades in
        // phase-aligned.
        if cf == 0.0 {
            self.osc[1].phase = self.osc[0].phase;
        } else if cf == 1.0 {
            self.osc[0].phase = self.osc[1].phase;
        }

        let fades = [1.0 - cf, cf];
        let mut sample = 0.0;
        let mut mod_out = 0.0;
        for (osc, fade) in self.osc.iter_mut().zip(fades) {
            let f = osc.freq_norm;
            osc.phase += f;
            osc.phase -= osc.phase.floor();

            let fade = fade * osc.nyquist_fade;
            let depth = ctx.cross_fm * osc.fm_rolloff;

            let mut s = 0.0;
            if fade > 0.0 {
                let mut p = osc.phase + mod_in;
                p -= p.floor();
                if let Some((warp, resolved)) = &ctx.phase {
                    p = warp.evaluate(p, resolved, f, &mut osc.warp);
                }
                s = sin_turns(p);
                if ctx.wave_dist.active() {
                    s = ctx.wave_dist.apply(s, f);
                }
                s *= fade;
            }
            mod_out += (s + 1.0) * depth;
            sample += s;
        }
        (sample * self.fade.advance(ctx.fade_inc), mod_out)
    }
}
