use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::core::mix::MixMode;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal};

fn default_channels() -> usize {
    1
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
pub struct MixDownParams {
    /// Polyphonic input whose channels are folded down.
    pub input: PolySignal,
    /// Target output channel count (1–64). Defaults to 1 (mono).
    #[serde(default = "default_channels")]
    #[deserr(default = default_channels())]
    pub channels: usize,
    /// How channels that land on the same output channel are combined.
    #[serde(default)]
    #[deserr(default)]
    mode: MixMode,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MixDownOutputs {
    /// Folded-down output: the input channels are panned evenly across the
    /// output field with an equal-power law (e.g. 3→2 ⇒ inputs at -1/0/+1).
    #[output(
        "output",
        "fold-down output: input channels panned evenly across the output channels",
        default
    )]
    sample: PolyOutput,
}

/// Folds a polyphonic input down to an arbitrary target channel count.
///
/// Unlike `$mix` (which mixes channel `n` of every input into output channel
/// `n`), `$mixDown` takes a single poly signal and distributes its `N` channels
/// across `M` output channels by panning them evenly over the output field with
/// an equal-power crossfade. With the default `channels = 1` it is a plain mono
/// sum; `channels = 2` produces an equal-power stereo fold-down. When the target
/// exceeds the input channel count, the input is instead spread to fill every
/// output channel (equal-power interpolation) so no channel is left silent.
///
/// - **channels** — target output channel count (1–64, default 1)
/// - **mode** — how channels landing on the same output combine (sum / average /
///   max / min); applies only when folding down (target ≤ input channels)
///
/// ## Example
///
/// ```js
/// // Fold a 3-voice spread down to stereo
/// $saw($spread(0, 5, 3)).mix(2).out()
/// ```
#[module(
    name = "$mixDown",
    channels_param = "channels",
    args(input, channels),
    has_init,
    patch_update
)]
pub struct MixDown {
    outputs: MixDownOutputs,
    params: MixDownParams,
    /// Equal-power pan gains, stored row-major as a flat `in_ch * out_ch`
    /// matrix: `gains[i * out_ch + j]` is the gain from input channel `i` to
    /// output channel `j`. Both counts are fixed for the patch's lifetime, so
    /// the matrix is sized to them in `init` (no fixed `PORT_MAX²` upfront cost)
    /// and rebuilt — never reallocated — in `on_patch_update`, then read
    /// unchanged on the audio thread.
    ///
    /// It lives in `channel_state` (not `state`) so the macro's size-aware
    /// transfer never grows a swapped-in box across a channel-count change: a
    /// fresh `init` always sizes each new instance, and `on_patch_update` only
    /// fills within that length.
    channel_state: Box<[f32]>,
}

message_handlers!(impl MixDown {});

impl MixDown {
    /// Size the gain matrix to the patch's input × output channel counts on the
    /// main thread, so `on_patch_update` (audio thread) only fills it. Both
    /// counts are resolved by construction time: `out_ch` from `channels`,
    /// `in_ch` from the input signal's width.
    fn init(&mut self, _sample_rate: f32) {
        let in_ch = self.params.input.channels().min(PORT_MAX_CHANNELS);
        let out_ch = self.channel_count();
        self.channel_state = vec![0.0; in_ch * out_ch].into_boxed_slice();
    }

    /// Fill `gains` with the equal-power pan matrix mapping `in_ch` (N) input
    /// channels onto `out_ch` (M) output channels. `gains[i][j]` is the gain
    /// from input channel `i` to output channel `j`.
    ///
    /// - `M == 1`: plain mono sum (every input contributes gain 1.0 to ch 0).
    /// - `M <= N` (fold-down): each input sits at normalized position
    ///   `t = i/(N-1)` (`0.5` if `N == 1`), mapped to output index space
    ///   `pos = t*(M-1)` and split between adjacent outputs with equal-power
    ///   gains `sqrt(1-frac)`/`sqrt(frac)`. Inputs that land on the same output
    ///   are combined by `mode` in `update()`.
    /// - `M > N` (fold-up): each *output* channel `j` sits at input-space
    ///   position `s = j*(N-1)/(M-1)` and gathers an equal-power interpolation
    ///   of the adjacent input channels. This fills every output channel — no
    ///   silent gaps. There are no collisions, so `mode` does not apply.
    fn build_gains(gains: &mut [f32], in_ch: usize, out_ch: usize) {
        // Clear any stale entries from a previous topology.
        for g in gains.iter_mut() {
            *g = 0.0;
        }

        if in_ch == 0 || out_ch == 0 {
            return;
        }

        if out_ch == 1 {
            for i in 0..in_ch {
                gains[i * out_ch] = 1.0;
            }
            return;
        }

        if out_ch <= in_ch {
            // Fold-down: scatter each input across the output field so every
            // input contributes.
            for i in 0..in_ch {
                let t = if in_ch == 1 {
                    0.5
                } else {
                    i as f32 / (in_ch - 1) as f32
                };
                let pos = t * (out_ch - 1) as f32;
                let lo = pos.floor() as usize;
                let frac = pos - lo as f32;
                gains[i * out_ch + lo] = (1.0 - frac).sqrt();
                if lo + 1 < out_ch {
                    gains[i * out_ch + lo + 1] = frac.sqrt();
                }
            }
        } else {
            // Fold-up: gather an equal-power interpolation of adjacent inputs
            // into each output so no output channel is left silent. With a
            // single input (N == 1) every output samples it at unity.
            for j in 0..out_ch {
                let s = if in_ch == 1 {
                    0.0
                } else {
                    j as f32 * (in_ch - 1) as f32 / (out_ch - 1) as f32
                };
                let lo = s.floor() as usize;
                let frac = s - lo as f32;
                gains[lo * out_ch + j] = (1.0 - frac).sqrt();
                if lo + 1 < in_ch {
                    gains[(lo + 1) * out_ch + j] = frac.sqrt();
                }
            }
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let in_ch = self.params.input.channels().min(PORT_MAX_CHANNELS);
        let out_ch = self.channel_count();

        // Snapshot input channel values once (stack, no allocation).
        let mut in_vals = [0.0f32; PORT_MAX_CHANNELS];
        for (i, v) in in_vals.iter_mut().enumerate().take(in_ch) {
            *v = self.params.input.get_value(i);
        }

        let gains = &self.channel_state;

        // Folding up gathers an equal-power crossfade per output (no inputs
        // collide on one output), so the combine mode is irrelevant — sum the
        // crossfade. Folding down can land several inputs on one output, where
        // `mode` decides how they combine. The mode match is hoisted out of the
        // per-output-channel loop so each branch carries only the accumulator
        // it needs — mirrors mix.rs.
        let mode = if out_ch > in_ch {
            MixMode::Sum
        } else {
            self.params.mode
        };
        match mode {
            MixMode::Sum => {
                for j in 0..out_ch {
                    let mut acc = 0.0f32;
                    for i in 0..in_ch {
                        acc += in_vals[i] * gains[i * out_ch + j];
                    }
                    self.outputs.sample.set(j, acc);
                }
            }
            MixMode::Average => {
                for j in 0..out_ch {
                    let mut acc = 0.0f32;
                    let mut count: u32 = 0;
                    for i in 0..in_ch {
                        let g = gains[i * out_ch + j];
                        if g == 0.0 {
                            continue;
                        }
                        acc += in_vals[i] * g;
                        count += 1;
                    }
                    self.outputs
                        .sample
                        .set(j, if count > 0 { acc / count as f32 } else { 0.0 });
                }
            }
            MixMode::Max => {
                for j in 0..out_ch {
                    let mut best_abs = -1.0f32;
                    let mut best_val = 0.0f32;
                    for i in 0..in_ch {
                        let g = gains[i * out_ch + j];
                        if g == 0.0 {
                            continue;
                        }
                        let v = in_vals[i] * g;
                        let av = v.abs();
                        // NaN comparisons are false, so NaN never displaces a
                        // finite best — matches mix.rs's no-panic semantics.
                        if av > best_abs {
                            best_abs = av;
                            best_val = v;
                        }
                    }
                    self.outputs
                        .sample
                        .set(j, if best_abs >= 0.0 { best_val } else { 0.0 });
                }
            }
            MixMode::Min => {
                for j in 0..out_ch {
                    let mut best_abs = f32::INFINITY;
                    let mut best_val = 0.0f32;
                    for i in 0..in_ch {
                        let g = gains[i * out_ch + j];
                        if g == 0.0 {
                            continue;
                        }
                        let v = in_vals[i] * g;
                        if v == 0.0 {
                            continue;
                        }
                        let av = v.abs();
                        if av < best_abs {
                            best_abs = av;
                            best_val = v;
                        }
                    }
                    self.outputs
                        .sample
                        .set(j, if best_abs.is_finite() { best_val } else { 0.0 });
                }
            }
        }
    }
}

impl crate::types::PatchUpdateHandler for MixDown {
    /// Build the equal-power gain matrix once per patch-apply. Both channel
    /// counts are fixed for the patch's lifetime, and this runs after the
    /// connect pass (so `input.channels()` is resolved) and after any
    /// `transfer_state_from`, so the unconditional rebuild overwrites a stale
    /// swapped-in matrix. The matrix was sized in `init`, so this only fills it
    /// — allocation-free, safe on the audio thread.
    fn on_patch_update(&mut self) {
        let in_ch = self.params.input.channels().min(PORT_MAX_CHANNELS);
        let out_ch = self.channel_count();
        Self::build_gains(&mut self.channel_state, in_ch, out_ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};

    /// Build a MixDown directly, initializing `_channel_count` and the output
    /// channels the way the module macro would. `channels` is clamped to
    /// 1–PORT_MAX_CHANNELS (matching the macro's `channels_param` codegen).
    fn make_mix_down(params: MixDownParams) -> MixDown {
        let channels = params.channels.clamp(1, PORT_MAX_CHANNELS);
        let mut outputs = MixDownOutputs::default();
        outputs.set_all_channels(channels);
        let mut m = MixDown {
            params,
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
            channel_state: Box::default(),
        };
        // Production constructs → init → connects → on_patch_update → process;
        // mirror that order so the gain matrix is sized (init) and built
        // (on_patch_update) before update() reads it.
        m.init(48000.0);
        crate::types::PatchUpdateHandler::on_patch_update(&mut m);
        m
    }

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "expected {b}, got {a}");
    }

    const HALF_POWER: f32 = std::f32::consts::FRAC_1_SQRT_2; // sqrt(0.5) ≈ 0.7071

    #[test]
    fn test_three_to_two_equal_power_pan() {
        // 3 input channels folded to 2: inputs land at -1, 0, +1.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(1.0), Signal::Volts(1.0)]),
            channels: 2,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        assert_eq!(m.outputs.sample.channels(), 2);
        // ch0 = 1*1 + 1*0.7071 + 1*0 ; ch1 = 1*0 + 1*0.7071 + 1*1
        approx(m.outputs.sample.get(0), 1.0 + HALF_POWER);
        approx(m.outputs.sample.get(1), HALF_POWER + 1.0);
    }

    #[test]
    fn test_center_input_equal_power() {
        // Only the center input is active: it pans equally to both outputs.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::poly(&[Signal::Volts(0.0), Signal::Volts(4.0), Signal::Volts(0.0)]),
            channels: 2,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        approx(m.outputs.sample.get(0), 4.0 * HALF_POWER);
        approx(m.outputs.sample.get(1), 4.0 * HALF_POWER);
    }

    #[test]
    fn test_mono_fold_down_default_channels() {
        // channels defaults to 1 in the DSL; here we exercise a plain mono sum.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(2.0), Signal::Volts(3.0)]),
            channels: 1,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        assert_eq!(m.outputs.sample.channels(), 1);
        approx(m.outputs.sample.get(0), 6.0);
    }

    #[test]
    fn test_default_channels_is_one() {
        // The serde/deserr default genuinely yields 1 (mono), not 0.
        let params: MixDownParams =
            deserr::deserialize::<MixDownParams, _, crate::param_errors::ModuleParamErrors>(
                serde_json::json!({ "input": 0.0 }),
            )
            .unwrap();
        assert_eq!(params.channels, 1);
    }

    #[test]
    fn test_single_input_fills_stereo() {
        // Target (2) > input channels (1): fold-up. The lone input fills both
        // output channels at unity (dual mono), leaving neither silent.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::mono(Signal::Volts(5.0)),
            channels: 2,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        assert_eq!(m.outputs.sample.channels(), 2);
        approx(m.outputs.sample.get(0), 5.0);
        approx(m.outputs.sample.get(1), 5.0);
    }

    #[test]
    fn test_average_mode() {
        // 3→2, average of the contributing (non-zero-gain) inputs per output.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(2.0), Signal::Volts(3.0)]),
            channels: 2,
            mode: MixMode::Average,
        });
        m.update(48000.0);
        // ch0 contributors: in0 (1*1=1), in1 (2*0.7071=1.4142) -> avg
        approx(m.outputs.sample.get(0), (1.0 + 2.0 * HALF_POWER) / 2.0);
        // ch1 contributors: in1 (2*0.7071=1.4142), in2 (3*1=3) -> avg
        approx(m.outputs.sample.get(1), (2.0 * HALF_POWER + 3.0) / 2.0);
    }

    #[test]
    fn test_channel_count_clamped() {
        // Over-large channel counts clamp to PORT_MAX_CHANNELS.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::poly(&[Signal::Volts(1.0), Signal::Volts(2.0)]),
            channels: PORT_MAX_CHANNELS + 50,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        assert_eq!(m.outputs.sample.channels(), PORT_MAX_CHANNELS);
    }

    #[test]
    fn test_target_larger_than_input_fills_all_channels() {
        // channels (4) > input channels (2): fold-up. Each output channel is an
        // equal-power interpolation of the adjacent inputs; no output is silent.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::poly(&[Signal::Volts(3.0), Signal::Volts(7.0)]),
            channels: 4,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        assert_eq!(m.outputs.sample.channels(), 4);
        // Output positions in input space: 0, 1/3, 2/3, 1.
        let w_near = (2.0f32 / 3.0).sqrt();
        let w_far = (1.0f32 / 3.0).sqrt();
        approx(m.outputs.sample.get(0), 3.0); // input 0 endpoint
        approx(m.outputs.sample.get(1), 3.0 * w_near + 7.0 * w_far);
        approx(m.outputs.sample.get(2), 3.0 * w_far + 7.0 * w_near);
        approx(m.outputs.sample.get(3), 7.0); // input 1 endpoint
        for ch in 0..4 {
            assert!(
                m.outputs.sample.get(ch).abs() > 0.0,
                "channel {ch} should not be silent"
            );
        }
    }

    #[test]
    fn test_mono_fills_all_channels() {
        // A single input channel spread to 4 fills every output at unity.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::mono(Signal::Volts(2.0)),
            channels: 4,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        for ch in 0..4 {
            approx(m.outputs.sample.get(ch), 2.0);
        }
    }

    #[test]
    fn test_zero_channels_clamps_to_mono() {
        // An explicit 0 still becomes 1 channel.
        let mut m = make_mix_down(MixDownParams {
            input: PolySignal::poly(&[Signal::Volts(2.0), Signal::Volts(4.0)]),
            channels: 0,
            mode: MixMode::Sum,
        });
        m.update(48000.0);
        assert_eq!(m.outputs.sample.channels(), 1);
        approx(m.outputs.sample.get(0), 6.0);
    }
}
