use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::PolyOutput;

fn default_channels() -> usize {
    1
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct NoiseParams {
    /// color of the noise: white, pink, brown
    #[serde(default)]
    #[deserr(default)]
    color: NoiseKind,
    /// number of independent noise channels (1–64), each a different sequence
    #[serde(default = "default_channels")]
    #[deserr(default = default_channels())]
    channels: usize,
}

#[derive(Clone, Copy, Deserr, JsonSchema, Debug, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[derive(Default)]
enum NoiseKind {
    /// equal energy across all frequencies
    #[default]
    White,
    /// rolled-off highs (−3 dB/octave), natural-sounding
    Pink,
    /// deep rumble (−6 dB/octave)
    Brown,
}

#[derive(Default)]
struct PinkFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
}

impl PinkFilter {
    fn process(&mut self, white: f32) -> f32 {
        self.b0 = 0.99886 * self.b0 + white * 0.0555179;
        self.b1 = 0.99332 * self.b1 + white * 0.0750759;
        self.b2 = 0.96900 * self.b2 + white * 0.153_852;
        self.b3 = 0.86650 * self.b3 + white * 0.3104856;
        self.b4 = 0.55000 * self.b4 + white * 0.5329522;
        self.b5 = -0.7616 * self.b5 - white * 0.0168980;
        self.b6 = white * 0.5362;

        let pink =
            self.b0 + self.b1 + self.b2 + self.b3 + self.b4 + self.b5 + self.b6 + white * 0.115926;
        (pink * 0.11).clamp(-1.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Default)]
struct LcgRng {
    state: u64,
}

impl LcgRng {
    fn next(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let bits = (self.state >> 32) as u32;
        let value = bits as f32 / u32::MAX as f32;
        value * 2.0 - 1.0
    }
}

/// Per-channel generator state. Each channel runs its own RNG, pink filter,
/// and brown integrator so every channel produces an independent sequence.
#[derive(Default)]
struct NoiseChannelState {
    generator: LcgRng,
    pink: PinkFilter,
    brown: f32,
}

/// Polyphonic noise generator with selectable color.
///
/// Generates random noise in one of three spectral colors:
/// - **White**: equal energy across all frequencies (bright, hissy)
/// - **Pink**: equal energy per octave (warm, balanced — good for "ocean" textures)
/// - **Brown**: steep low-frequency emphasis (deep, rumbling)
///
/// The `channels` parameter sets how many independent channels are generated;
/// each channel runs its own RNG seeded distinctly, so every channel is a
/// different random sequence.
///
/// Output range is **±5V**.
///
/// ## Example
///
/// ```js
/// $noise("pink").out()
/// $noise("white", { channels: 4 }).out()
/// ```
#[module(name = "$noise", channels_param = "channels", patch_update, args(color))]
pub struct Noise {
    outputs: NoiseOutputs,
    params: NoiseParams,
    state: NoiseState,
    /// Per-channel generator state, one element per polyphonic channel.
    channel_state: Box<[NoiseChannelState]>,
}

/// Module-level state for the Noise module.
#[derive(Default)]
struct NoiseState {
    last_noise_type: NoiseKind,
    /// Set once the per-channel RNGs have been seeded off the module's stable
    /// heap address (see `seed`).
    seeded: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct NoiseOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

impl Noise {
    /// Seed each channel's RNG off this module's address so the sequences are
    /// stable for a given instance yet differ between instances. Mixing the
    /// channel index with an odd golden-ratio constant decorrelates the
    /// channels, giving each a different random sequence.
    ///
    /// Called from `on_patch_update`, not `init`: `init` runs while the module
    /// still lives in a transient stack slot (it is moved into its boxed heap
    /// home afterwards). That slot is reused across constructions, so every
    /// instance would capture the same address and seed identically. By
    /// `on_patch_update` the module sits at its stable per-instance heap
    /// address, so `self as *const Self` differs between instances.
    ///
    /// Guarded by `seeded` (which, with the RNG state, rides
    /// `transfer_state_from`): `on_patch_update` runs *after* the state swap, so
    /// re-seeding unconditionally would clobber the sequence carried over from
    /// the previous patch on every edit. Seeding once keeps each node's stream
    /// continuous across patch updates.
    fn seed(&mut self) {
        let base = self as *const Self as usize as u64;
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            state.generator.state = base ^ (ch as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        self.state.seeded = true;
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let color = self.params.color;

        // Color change resets every channel's stateful filters so the new
        // spectrum starts clean.
        if self.state.last_noise_type != color {
            self.state.last_noise_type = color;
            for state in self.channel_state.iter_mut() {
                state.pink.reset();
                state.brown = 0.0;
            }
        }

        for ch in 0..channels {
            let state = &mut self.channel_state[ch];
            let white = state.generator.next();
            let colored = match color {
                NoiseKind::White => white,
                NoiseKind::Pink => state.pink.process(white),
                NoiseKind::Brown => {
                    state.brown = (state.brown + white * 0.02).clamp(-1.0, 1.0);
                    state.brown
                }
            };
            self.outputs.sample.set(ch, colored.clamp(-1.0, 1.0) * 5.0);
        }
    }
}

impl crate::types::PatchUpdateHandler for Noise {
    fn on_patch_update(&mut self) {
        if !self.state.seeded {
            self.seed();
        }
    }
}

message_handlers!(impl Noise {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, PatchUpdateHandler};

    /// Build a Noise module with the given params, mirroring the production
    /// lifecycle: the macro allocates `channel_state` to the channel count, then
    /// `on_patch_update` seeds each channel's RNG.
    fn make_noise(params: NoiseParams) -> Noise {
        let channels = __noise_derive_channel_count(&params);
        let mut outputs = NoiseOutputs::default();
        outputs.set_all_channels(channels);
        let mut n = Noise {
            params,
            outputs,
            state: NoiseState::default(),
            channel_state: (0..channels)
                .map(|_| NoiseChannelState::default())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        n.on_patch_update();
        n
    }

    #[test]
    fn channels_param_sets_channel_count() {
        let params = NoiseParams {
            color: NoiseKind::White,
            channels: 4,
        };
        assert_eq!(__noise_derive_channel_count(&params), 4);
    }

    #[test]
    fn channels_clamped() {
        let zero = NoiseParams {
            color: NoiseKind::White,
            channels: 0,
        };
        assert_eq!(__noise_derive_channel_count(&zero), 1);
        let huge = NoiseParams {
            color: NoiseKind::White,
            channels: 1000,
        };
        assert_eq!(
            __noise_derive_channel_count(&huge),
            crate::poly::PORT_MAX_CHANNELS
        );
    }

    #[test]
    fn channels_produce_different_sequences() {
        let mut n = make_noise(NoiseParams {
            color: NoiseKind::White,
            channels: 4,
        });
        // Collect a few samples per channel.
        let mut series = [[0.0f32; 16]; 4];
        for s in series.iter_mut() {
            n.update(48000.0);
            for (ch, slot) in s.iter_mut().enumerate() {
                *slot = n.outputs.sample.get(ch);
            }
        }
        // No two channels should produce an identical run (seeds differ).
        for a in 0..4 {
            for b in (a + 1)..4 {
                let same = (0..16).all(|i| (series[i][a] - series[i][b]).abs() < 1e-9);
                assert!(!same, "channels {a} and {b} produced identical sequences");
            }
        }
    }

    #[test]
    fn output_bounded() {
        let mut n = make_noise(NoiseParams {
            color: NoiseKind::Pink,
            channels: 3,
        });
        for _ in 0..1000 {
            n.update(48000.0);
            for ch in 0..3 {
                let v = n.outputs.sample.get(ch);
                assert!(v.abs() <= 5.01, "channel {ch} output {v} out of range");
            }
        }
    }
}
