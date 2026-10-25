//! `$multimode` — one channel of Mutable Instruments Blades, modelled from its
//! published schematic (blades_v40).
//!
//! Signal path: drive VCA → zener-clamped transimpedance amp → clean/wavefolded
//! crossfade → 2164-based state-variable filter → LP/BP/HP morphing VCAs.
//! Every V2164 control port here is driven through a linearising servo or a
//! precision rectifier whose diode keeps the control voltage ≥ 0 V, so each
//! VCA's gain is clamped to at most unity except the drive VCA, which has no
//! such diode.

mod components;
mod controls;
mod curve;
mod drive;
mod group;
mod lanes;
mod output;
mod svf;
mod vca;

use std::ops::Range;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::rng::seed_base,
    poly::{PolyOutput, PolySignal},
};

use controls::{ControlInputs, update_controls};
use group::{GroupOutputs, GroupState, Tables, process_block_group, process_group, step};
use lanes::{HalfbandDown, LANES, f32x4};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct MultimodeParams {
    /// signal input (DC-coupled)
    input: PolySignal,
    /// cutoff frequency in V/Oct (0V = C4), capped at ~21 kHz
    #[signal(type = frequency, default = 0.0, range = (-5.0, 5.0))]
    cutoff: PolySignal,
    /// resonance (0-5). Self-oscillates from ~4.3.
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    resonance: Option<PolySignal>,
    /// response: 0 = lowpass, 2.5 = bandpass, 5 = highpass, crossfading between
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    mode: Option<PolySignal>,
    /// input drive (0 = unity, 5 = +18 dB), cubic response
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    drive: Option<PolySignal>,
    /// drive response (0 = soft clipping, 5 = two-stage wavefolder)
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    fold: Option<PolySignal>,
    /// when true (default), the circuit runs at twice the engine rate, which
    /// reduces aliasing from the clipper, wavefolder and self-oscillation and
    /// keeps the highpass accurate at high cutoffs; false costs less CPU
    #[serde(default = "default_true")]
    #[deserr(default = default_true())]
    high_res: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MultimodeOutputs {
    #[output("output", "response selected by mode", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
    #[output("lp", "lowpass (the output at mode 0)", range = (-10.0, 10.0))]
    lp: PolyOutput,
    #[output("bp", "bandpass (the output near mode 2.5)", range = (-10.0, 10.0))]
    bp: PolyOutput,
    #[output("hp", "highpass (the output at mode 5)", range = (-10.0, 10.0))]
    hp: PolyOutput,
}

/// Mutable Instruments Blades–style multimode filter with drive and wavefolder.
///
/// A circuit model of one channel of Blades: a 2164 VCA state-variable filter
/// that morphs continuously from lowpass through bandpass to highpass, preceded
/// by a drive VCA into a zener soft clipper and a two-stage diode wavefolder.
/// The signal path is DC-coupled, so it can process CVs too.
///
/// - **cutoff** — V/Oct. Tracks exactly at low cutoffs and runs progressively
///   flat in the top octaves (−15 cents at 1 kHz, about half a semitone at
///   5 kHz), topping out near 21 kHz, as the circuit does. Responds at audio
///   rate, so an oscillator patched in gives exponential FM.
/// - **resonance** — 0–5. Self-oscillates above ~4.3; the bandpass is then
///   held near ±4 V by the soft knee of the integrator's zeners. With no input,
///   `mode` shifts the phase of the sine.
/// - **mode** — 0 lowpass, 2.5 bandpass, 5 highpass; values between crossfade.
/// - **drive** — input gain ahead of everything else: 0 is unity, 5 is about
///   +18 dB. The zeners' soft knee already rounds a ±5 V input at 0.
/// - **fold** — 0 soft clipping only, 5 the two-stage wavefolder.
/// - **highRes** — on by default. Off runs the circuit at the engine rate for
///   less CPU, with more aliasing from hot drive, folding and self-oscillation,
///   and a highpass that attenuates more below the cutoff once the cutoff is
///   above ~8 kHz (about 2 dB more at 15 kHz).
///
/// Passband gain in lowpass mode is about +2.8 dB at unity drive, rolling off
/// above ~20 kHz at the output amp, whose zeners compress the output softly
/// above about 6 V.
///
/// ```js
/// // resonant sweep that morphs from lowpass to highpass
/// $multimode($saw('c2'), $sine('0.1hz').range(-1, 2), 3.8, $sine('0.07hz').range(0, 5)).out()
/// // wavefolded sine through a gentle lowpass
/// $multimode($sine('c3'), 'c6', 1, 0, { drive: 3, fold: 3 }).out()
/// // no input: a self-oscillating sine at A3
/// $multimode(0, 'a3', 5).out()
/// // without oversampling, for less CPU
/// $multimode($saw('c2'), 'c5', 2, 0, { highRes: false }).out()
/// ```
#[module(
    name = "$multimode",
    args(input, cutoff, resonance, mode),
    has_init,
    patch_update,
    block
)]
pub struct Multimode {
    outputs: MultimodeOutputs,
    state: MultimodeState,
    channel_state: Box<[GroupState]>,
    params: MultimodeParams,
}

#[derive(Default)]
struct MultimodeState {
    sample_rate: f32,
    /// Channel count the groups' lanes were last set up for.
    channels: usize,
}

/// `read(channel)` for the first `active` channels from `base`, 0 for dead
/// lanes.
#[inline]
fn lanes(base: usize, active: usize, read: impl Fn(usize) -> f32) -> [f32; LANES] {
    std::array::from_fn(|lane| {
        if lane < active {
            read(base + lane)
        } else {
            0.0
        }
    })
}

impl Multimode {
    fn init(&mut self, sample_rate: f32) {
        components::prime();
        drive::prime();
        output::prime();
        self.state.sample_rate = sample_rate;
        let base = seed_base(self);
        for (group, state) in self.channel_state.iter_mut().enumerate() {
            for (lane, noise) in state.noise.iter_mut().enumerate() {
                noise.seed(base, group * LANES + lane);
            }
        }
    }

    fn core_rate(&self) -> f32 {
        if self.params.high_res {
            2.0 * self.state.sample_rate
        } else {
            self.state.sample_rate
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let (rate, high_res) = (self.core_rate(), self.params.high_res);
        let tables = Tables::get();
        let (p, outputs) = (&self.params, &mut self.outputs);
        let write =
            |outputs: &mut MultimodeOutputs, base: usize, active: usize, out: GroupOutputs| {
                let [sample, lp, bp, hp] = out;
                for lane in 0..active {
                    let ch = base + lane;
                    outputs.sample.set(ch, sample[lane]);
                    outputs.lp.set(ch, lp[lane]);
                    outputs.bp.set(ch, bp[lane]);
                    outputs.hp.set(ch, hp[lane]);
                }
            };
        // A single channel runs on lane 0 alone, without three dead lanes.
        if channels == 1 {
            let state = &mut self.channel_state[0];
            update_controls::<f32>(&mut state.controls, &ControlInputs::read(p, 0, 1), 1, rate);
            let input = lanes(0, 1, |ch| p.input.get_value(ch));
            let out = step::<f32>(state, input, 1, high_res, tables);
            write(outputs, 0, 1, out);
            return;
        }
        for (group, state) in self
            .channel_state
            .iter_mut()
            .take(channels.div_ceil(LANES))
            .enumerate()
        {
            let base = group * LANES;
            let active = (channels - base).min(LANES);
            let controls = ControlInputs::read(p, base, active);
            let input = lanes(base, active, |ch| p.input.get_value(ch));
            let group = (active, high_res, rate, tables);
            // A group's one live channel runs on lane 0 as well.
            let out = if active == 1 {
                process_group::<f32>(state, &controls, input, group)
            } else {
                process_group::<f32x4>(state, &controls, input, group)
            };
            write(outputs, base, active, out);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut MultimodeBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let (rate, high_res) = (self.core_rate(), self.params.high_res);
        let tables = Tables::get();
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), channels, &mut scratch)?;
        for (group, state) in self
            .channel_state
            .iter_mut()
            .take(channels.div_ceil(LANES))
            .enumerate()
        {
            let base = group * LANES;
            let active = (channels - base).min(LANES);
            let group = (base, active, high_res, rate, tables);
            // A group's one live channel runs on lane 0, as in `update`.
            if active == 1 {
                process_block_group::<f32>(state, &inputs, outputs, range.clone(), group);
            } else {
                process_block_group::<f32x4>(state, &inputs, outputs, range.clone(), group);
            }
        }
        Some(())
    }
}

impl crate::types::PatchUpdateHandler for Multimode {
    fn on_patch_update(&mut self) {
        let rate = self.core_rate();
        let (old, channels) = (self.state.channels, self.channel_count());
        let groups = channels.div_ceil(LANES);
        for group in self.channel_state.iter_mut().take(groups) {
            group.configure(rate);
        }
        // A group with one live channel keeps that channel's four output
        // downsamplers in one downsampler's lanes; a group moving between one
        // live channel and several transposes them. A group that comes back
        // with one live channel starts all four from rest, since clearing its
        // lane alone would leave the other three outputs' history.
        let live = |count: usize, group: usize| count.saturating_sub(group * LANES).min(LANES);
        for (group, state) in self.channel_state.iter_mut().enumerate() {
            let (before, after) = (live(old, group), live(channels, group));
            if before != 0 && after != 0 && (before == 1) != (after == 1) {
                HalfbandDown::transpose(&mut state.down);
            } else if before == 0 && after == 1 {
                state.down = Default::default();
            }
        }
        // Channels new to this patch start from rest, as they would in a fresh
        // module, rather than from a lane that ran dead before.
        for ch in old.min(channels)..channels {
            self.channel_state[ch / LANES].clear_lane(ch % LANES);
        }
        // A new channel count changes which lanes are dead; recomputing every
        // law sets them to follow lane 0 again.
        if old != channels {
            for group in self.channel_state.iter_mut().take(groups) {
                group.controls.clear();
            }
        }
        self.state.channels = channels;
    }
}

message_handlers!(impl Multimode {});

#[cfg(test)]
mod tests;
