use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::rng::{hash_bipolar, seed_base};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal};

mod channel;
mod generators;
#[cfg(test)]
mod tests;

use channel::NoiseChannelState;
use generators::{LFSR_LONG_PERIOD, LFSR_SHORT_CYCLE, LfsrLoop, LfsrPhases, lfsr_even_phase};

fn default_channels() -> usize {
    1
}

/// At least `channels`, widened to match a polyphonic `rate`.
fn noise_derive_channel_count(params: &NoiseParams) -> usize {
    let rate_channels = params.rate.as_ref().map_or(0, PolySignal::channels);
    params
        .channels
        .max(rate_channels)
        .clamp(1, PORT_MAX_CHANNELS)
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct NoiseParams {
    /// kind of noise: white, pink, brown, blue, violet, binary, gaussian,
    /// lfsr, lfsrShort, velvet, telegraph, dust
    #[serde(default)]
    #[deserr(default)]
    color: NoiseKind,
    /// generator clock in V/Oct (use `$hz()` or a note name); a new value is drawn
    /// each tick. For telegraph and dust it is the average events per second,
    /// and for lfsrShort the pitch of its 93-step loop.
    /// Unconnected, the generator runs at the sample rate.
    #[signal(type = pitch)]
    #[deserr(default)]
    rate: Option<PolySignal>,
    /// how the output moves between ticks: step, linear, smooth
    #[serde(default)]
    #[deserr(default)]
    interp: NoiseInterp,
    /// number of independent noise channels (1–64), each a different sequence;
    /// a polyphonic `rate` raises it to that many channels
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
    /// boosted highs (+3 dB/octave)
    Blue,
    /// strongly boosted highs (+6 dB/octave), thin and airy
    Violet,
    /// only −5V or +5V, a coin flip each tick
    Binary,
    /// bell-curve values clustered around 0V (σ ≈ 1.67V)
    Gaussian,
    /// 15-bit shift-register noise (NES-style), ±5V
    Lfsr,
    /// 93-step shift-register loop: pitched, metallic, ±5V
    LfsrShort,
    /// one ±5V impulse at a random point in every tick
    Velvet,
    /// ±5V that flips sign at random moments, `rate` times per second on average
    Telegraph,
    /// rests at −5V with single-sample +5V triggers at random moments, `rate`
    /// per second on average
    Dust,
}

#[derive(Clone, Copy, Deserr, JsonSchema, Debug, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[derive(Default)]
enum NoiseInterp {
    /// hold each value until the next tick
    #[default]
    Step,
    /// ramp straight from one value to the next
    Linear,
    /// curve through the values with no corners
    Smooth,
}

/// Polyphonic noise generator with selectable color and clock rate.
///
/// Spectral colors:
/// - **white**: equal energy across all frequencies (bright, hissy)
/// - **pink**: equal energy per octave (warm, balanced — good for "ocean" textures)
/// - **brown**: steep low-frequency emphasis (deep, rumbling)
/// - **blue** / **violet**: high-frequency emphasis (+3 / +6 dB/octave), for hats and air
///
/// Distributions and digital noise:
/// - **binary**: only −5V or +5V — at the same peak it is louder than white
/// - **gaussian**: values cluster near 0V with occasional large excursions
/// - **lfsr** / **lfsrShort**: NES-style shift-register noise; lfsr repeats every 32767 ticks,
///   lfsrShort every 93, which sounds pitched and metallic — its `rate` sets that pitch
///
/// Impulse and switching noise:
/// - **velvet**: exactly one ±5V impulse per tick, at a random moment within it
/// - **telegraph**: ±5V that flips at random moments, on average `rate` times per second
/// - **dust**: rests at −5V with single-sample +5V triggers at random moments, on average `rate` per second
///
/// `rate` clocks the generator: the color is computed at that rate, so slow
/// rates give random CV (a slow brown is a drunk walk) and audio rates give
/// lo-fi, band-limited noise. `interp` shapes the output between ticks;
/// `linear` and `smooth` turn any color into a smooth random LFO. Unconnected,
/// the generator draws a new value every sample, and telegraph and dust switch
/// on half of all samples.
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
/// $noise("lfsrShort", { rate: "c3" }).out()
/// $sine($noise("brown", { rate: $hz(4), interp: "smooth" })).out()
/// ```
#[module(
    name = "$noise",
    channels_derive = noise_derive_channel_count,
    has_init,
    patch_update,
    args(color)
)]
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
    inv_sample_rate: f32,
    /// Random per-module seed that places the LFSR channels, drawn when the
    /// module's first channels are seeded. It transfers with the module, so
    /// channels a patch update adds play short-mode cycles distinct from the
    /// transferred channels'.
    lfsr_seed: u32,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct NoiseOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

impl Noise {
    fn init(&mut self, sample_rate: f32) {
        self.state.inv_sample_rate = 1.0 / sample_rate;
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let color = self.params.color;

        // Unconnected, the clock ticks every sample with nothing to
        // interpolate, and telegraph and dust switch with the maximum-entropy
        // probability 1/2.
        let rate = self.params.rate.as_ref();
        let interp = if rate.is_some() {
            self.params.interp
        } else {
            NoiseInterp::Step
        };
        let ticks_per_cycle = if color == NoiseKind::LfsrShort {
            LFSR_SHORT_CYCLE as f32
        } else {
            1.0
        };

        for ch in 0..channels {
            // Rate cycles per sample, capped at one.
            let cycles = rate
                .map(|rate| (voct_to_hz(rate.get_value(ch)) * self.state.inv_sample_rate).min(1.0));
            let state = &mut self.channel_state[ch];
            let value = match color {
                NoiseKind::Telegraph => state.telegraph(cycles.unwrap_or(0.5)),
                NoiseKind::Dust => state.dust(cycles.unwrap_or(0.5)),
                NoiseKind::Velvet => state.velvet(cycles.unwrap_or(1.0)),
                _ => state.clocked(color, cycles.map_or(1.0, |c| c * ticks_per_cycle), interp),
            };
            self.outputs.sample.set(ch, value * 5.0);
        }
    }
}

impl crate::types::PatchUpdateHandler for Noise {
    /// Restarts every channel when the color changed, and seeds channels that
    /// have not been seeded yet: every channel of a new module, or those a
    /// patch update added beside transferred ones.
    ///
    /// Runs here, not in `init`, because `init` runs while the module still
    /// lives in a transient stack slot reused across constructions, so every
    /// instance would seed identically. By `on_patch_update` the module sits at
    /// its stable per-instance heap address. Transferred channels keep their
    /// seeded RNG, so each stream stays continuous across patch updates.
    fn on_patch_update(&mut self) {
        self.prepare_channels(seed_base(self));
    }
}

impl Noise {
    /// The body of `on_patch_update`, seeding new channels from `base`.
    fn prepare_channels(&mut self, base: u64) {
        let color = self.params.color;
        let color_changed = self.state.last_noise_type != color;
        self.state.last_noise_type = color;
        let fresh = !self.channel_state.iter().any(|state| state.seeded);
        if fresh {
            let unit = (hash_bipolar(base, 0) + 1.0) * 0.5;
            self.state.lfsr_seed = (unit * f64::from(u32::MAX)) as u32;
        }
        let lfsr_seed = self.state.lfsr_seed;
        let channels = self.channel_state.len();
        // Channels added beside running ones claim long-mode phases away from
        // the current phases of the running channels.
        let mut phases = LfsrPhases::new(
            self.channel_state
                .iter()
                .filter(|state| state.seeded)
                .map(NoiseChannelState::lfsr_phase),
        );
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            if !state.seeded {
                let phase = if fresh {
                    lfsr_even_phase(lfsr_seed % LFSR_LONG_PERIOD, ch, channels)
                } else {
                    phases.claim()
                };
                let lfsr_loop = LfsrLoop::for_channel(lfsr_seed, ch);
                state.seed(base, ch, phase, lfsr_loop, color);
            } else if color_changed {
                state.reset(color);
            }
        }
    }
}

message_handlers!(impl Noise {});
