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

/// The order the fold (`f`), cheby (`c`) and segment (`s`) warps run in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, Serialize, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum EffectOrder {
    #[default]
    Fcs,
    Fsc,
    Cfs,
    Csf,
    Sfc,
    Scf,
}

#[derive(Clone, Copy)]
enum Warp {
    Fold,
    Cheby,
    Segment,
}

impl EffectOrder {
    fn stages(self) -> [Warp; 3] {
        use Warp::*;
        match self {
            EffectOrder::Fcs => [Fold, Cheby, Segment],
            EffectOrder::Fsc => [Fold, Segment, Cheby],
            EffectOrder::Cfs => [Cheby, Fold, Segment],
            EffectOrder::Csf => [Cheby, Segment, Fold],
            EffectOrder::Sfc => [Segment, Fold, Cheby],
            EffectOrder::Scf => [Segment, Cheby, Fold],
        }
    }
}

/// The fold/cheby/segment chain applied to every oscillator's sine. Each amount
/// is a 0–1 knob; a stage at 0 is the identity and is skipped.
#[derive(Clone, Copy)]
pub(super) struct WarpChain {
    stages: [Warp; 3],
    /// Whether any stage is non-zero.
    pub active: bool,
    fold: f32,
    cheby: f32,
    segment: f32,
}

impl WarpChain {
    pub fn new(order: EffectOrder, fold: f32, cheby: f32, segment: f32) -> Self {
        Self {
            stages: order.stages(),
            active: fold > 0.0 || cheby > 0.0 || segment > 0.0,
            fold,
            cheby,
            segment,
        }
    }

    // Kept out of line: inlined, the table lookups bloat the oscillator loop
    // and slow it down even while every stage is off.
    #[inline(never)]
    fn apply(&self, mut x: f32, freq_norm: f32) -> f32 {
        for stage in self.stages {
            x = match stage {
                Warp::Fold if self.fold > 0.0 => {
                    lookup_fold(x, aa_fold(freq_norm, fold_amount(self.fold)))
                }
                Warp::Cheby if self.cheby > 0.0 => {
                    interpolate_cheby(x, aa_cheby(freq_norm, self.cheby))
                }
                Warp::Segment if self.segment > 0.0 => {
                    interpolate_segment(x, aa_segment(freq_norm, self.segment))
                }
                _ => x,
            };
        }
        x
    }
}

/// Settings shared by every voice of a channel for one sample.
pub(super) struct VoiceContext<'a> {
    pub inv_sample_rate: f32,
    /// The phase warp with its stage values for this channel.
    pub phase: Option<(&'a PhaseWarp, ResolvedWarp)>,
    pub warps: WarpChain,
    /// Cross-FM depth, before per-oscillator antialiasing.
    pub cross_fm: f32,
    /// How much of each interval is a hard step rather than a crossfade, 0–0.5.
    pub crossfade_factor: f32,
    /// Per-sample coefficient of the crossfade lowpass.
    pub crossfade_coef: f32,
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
    /// Lowpassed crossfade; `NaN` until the voice first sounds, so it starts on
    /// its target.
    crossfade: f32,
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

    /// Advance one sample. `pitches` are the pair's V/Oct pitches, `crossfade`
    /// the scale position between them, and `mod_in` the incoming cross-FM
    /// phase offset. Returns the voice's sample in `[-1, 1]` and its cross-FM
    /// output.
    #[inline]
    pub fn process(
        &mut self,
        pitches: [f32; 2],
        crossfade: f32,
        mod_in: f32,
        ctx: &VoiceContext,
    ) -> (f32, f32) {
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
        let cf = if self.crossfade.is_nan() {
            target
        } else {
            let next = self.crossfade + (target - self.crossfade) * ctx.crossfade_coef;
            if (target - next).abs() < 1e-6 {
                target
            } else {
                next
            }
        };
        self.crossfade = cf;

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
        for ((osc, pitch), fade) in self.osc.iter_mut().zip(pitches).zip(fades) {
            if pitch != osc.pitch {
                let f = voct_to_hz(pitch) * ctx.inv_sample_rate;
                let rolloff = (1.0 - 2.0 * f).max(0.0);
                osc.pitch = pitch;
                osc.freq_norm = f;
                osc.nyquist_fade = (1.0 - 4.0 * f * f).max(0.0);
                osc.fm_rolloff = rolloff * rolloff;
            }
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
                if ctx.warps.active {
                    s = ctx.warps.apply(s, f);
                }
                s *= fade;
            }
            mod_out += (s + 1.0) * depth;
            sample += s;
        }
        (sample * self.fade.advance(ctx.fade_inc), mod_out)
    }
}
