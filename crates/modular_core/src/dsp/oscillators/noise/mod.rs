use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal};

mod generators;
#[cfg(test)]
mod tests;

use generators::{
    BLUE_GAIN, BROWN_LEAK, BROWN_STEP, Lfsr, PINK_GAIN, PinkFilter, catmull_rom, gaussian,
};

fn default_channels() -> usize {
    1
}

/// At least `channels`, widened to match a polyphonic `rate`.
fn noise_derive_channel_count(params: &NoiseParams) -> usize {
    let rate_channels = params.rate.as_ref().map_or(0, PolySignal::channels);
    params
        .channels
        .max(rate_channels)
        .clamp(1, crate::poly::PORT_MAX_CHANNELS)
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
    /// each tick. For telegraph and dust it is the average events per second.
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

/// Per-channel generator state. Each channel runs its own RNG and filters so
/// every channel produces an independent sequence.
#[derive(Default)]
struct NoiseChannelState {
    generator: LcgRng,
    pink: PinkFilter,
    /// Previous raw pink value, differentiated for blue.
    pink_prev: f32,
    /// Previous white value, differentiated for violet.
    white_prev: f32,
    brown: f32,
    lfsr: Lfsr,
    /// Clock phase in `[0, 1)`; a tick fires each time it wraps.
    phase: f32,
    /// The last four drawn values, oldest first.
    history: [f32; 4],
    /// Telegraph level: +1 when set, −1 otherwise.
    telegraph_high: bool,
    /// Velvet: clock phase at which this tick's impulse fires.
    impulse_at: f32,
    /// Velvet: sign of this tick's impulse.
    impulse_sign: f32,
    /// Velvet: this tick's impulse has not fired yet.
    impulse_pending: bool,
}

impl NoiseChannelState {
    /// Clear filter and clock state so a new kind starts clean.
    fn reset(&mut self) {
        self.pink = PinkFilter::default();
        self.pink_prev = 0.0;
        self.white_prev = 0.0;
        self.brown = 0.0;
        self.phase = 0.0;
        self.history = [0.0; 4];
        self.impulse_pending = false;
        self.lfsr.reset(&mut self.generator);
    }

    /// Draw the next value of a clocked kind, in `[-1, 1]`.
    fn draw(&mut self, kind: NoiseKind) -> f32 {
        let rng = &mut self.generator;
        match kind {
            NoiseKind::White => rng.next_bipolar(),
            NoiseKind::Pink => (self.pink.process(rng.next_bipolar()) * PINK_GAIN).clamp(-1.0, 1.0),
            NoiseKind::Brown => {
                self.brown =
                    (self.brown * BROWN_LEAK + rng.next_bipolar() * BROWN_STEP).clamp(-1.0, 1.0);
                self.brown
            }
            NoiseKind::Blue => {
                let pink = self.pink.process(rng.next_bipolar());
                let blue = (pink - self.pink_prev) * BLUE_GAIN;
                self.pink_prev = pink;
                blue.clamp(-1.0, 1.0)
            }
            NoiseKind::Violet => {
                let white = rng.next_bipolar();
                let violet = (white - self.white_prev) * 0.5;
                self.white_prev = white;
                violet
            }
            NoiseKind::Binary => {
                if rng.next_unit() < 0.5 {
                    -1.0
                } else {
                    1.0
                }
            }
            NoiseKind::Gaussian => gaussian(rng),
            NoiseKind::Lfsr => self.lfsr.step(1),
            NoiseKind::LfsrShort => self.lfsr.step(6),
            NoiseKind::Velvet | NoiseKind::Telegraph | NoiseKind::Dust => 0.0,
        }
    }

    /// Advance the clock by `inc` (≤ 1) and return the output for a clocked
    /// value kind.
    fn clocked(&mut self, kind: NoiseKind, inc: f32, interp: NoiseInterp) -> f32 {
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            let next = self.draw(kind);
            self.history = [self.history[1], self.history[2], self.history[3], next];
        }
        let h = self.history;
        match interp {
            NoiseInterp::Step => h[3],
            NoiseInterp::Linear => h[2] + (h[3] - h[2]) * self.phase,
            NoiseInterp::Smooth => catmull_rom(h, self.phase).clamp(-1.0, 1.0),
        }
    }

    /// Advance the clock by `inc` (≤ 1) and return the velvet output. Each
    /// tick schedules one impulse at a random phase within it; an impulse
    /// still pending when its tick ends fires on that final sample.
    fn velvet(&mut self, inc: f32) -> f32 {
        let mut out = 0.0;
        self.phase += inc;
        if self.phase >= 1.0 {
            if self.impulse_pending {
                out = self.impulse_sign;
            }
            self.phase -= 1.0;
            self.impulse_at = self.generator.next_unit();
            self.impulse_sign = if self.generator.next_unit() < 0.5 {
                -1.0
            } else {
                1.0
            };
            self.impulse_pending = true;
        }
        if self.impulse_pending && self.phase > self.impulse_at {
            self.impulse_pending = false;
            out = self.impulse_sign;
        }
        out
    }
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
/// - **lfsr** / **lfsrShort**: NES-style shift-register noise; short mode is pitched and metallic
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
/// $noise("lfsrShort", { rate: "c6" }).out()
/// $sine($noise("brown", { rate: $hz(4), interp: "smooth" })).out()
/// ```
#[module(
    name = "$noise",
    channels_derive = noise_derive_channel_count,
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
        let base = seed_base(self);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            state.generator.seed(base, ch);
            state.reset();
        }
        self.state.seeded = true;
    }

    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let color = self.params.color;
        let interp = self.params.interp;
        let inv_sample_rate = 1.0 / sample_rate;

        if self.state.last_noise_type != color {
            self.state.last_noise_type = color;
            for state in self.channel_state.iter_mut() {
                state.reset();
            }
        }

        for ch in 0..channels {
            // Clock advance per sample, capped at one tick per sample.
            let inc = self.params.rate.as_ref().map_or(1.0, |rate| {
                (voct_to_hz(rate.get_value(ch)) * inv_sample_rate).min(1.0)
            });
            let state = &mut self.channel_state[ch];
            let value = match color {
                NoiseKind::Telegraph | NoiseKind::Dust => {
                    // Per-sample event probability; unconnected, the maximum-entropy 1/2.
                    let p = if self.params.rate.is_some() { inc } else { 0.5 };
                    let event = state.generator.next_unit() < p;
                    if color == NoiseKind::Dust {
                        if event { 1.0 } else { -1.0 }
                    } else {
                        state.telegraph_high ^= event;
                        if state.telegraph_high { 1.0 } else { -1.0 }
                    }
                }
                NoiseKind::Velvet => state.velvet(inc),
                _ if self.params.rate.is_none() => state.clocked(color, 1.0, NoiseInterp::Step),
                _ => state.clocked(color, inc, interp),
            };
            self.outputs.sample.set(ch, value * 5.0);
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
