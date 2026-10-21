use std::f32::consts::{FRAC_PI_2, PI, TAU};

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::{rng::hash_bipolar, voct_to_hz, wrap_phase};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

#[cfg(test)]
mod tests;

/// Most wave components one sea sums.
const MAX_WAVES: usize = 16;

/// Total steepness at 5V. Below 1 the surface never folds over itself; the
/// margin keeps crest slopes finite and the gauge solve well-conditioned.
const MAX_STEEPNESS: f32 = 0.95;
const STEEPNESS_PER_VOLT: f32 = MAX_STEEPNESS / 5.0;
const CHOP_OCTAVES_PER_VOLT: f32 = 0.5;
const SPREAD_RADIANS_PER_VOLT: f32 = PI / 5.0;

const DEFAULT_STEEPNESS: f32 = 3.0;
const DEFAULT_CHOP: f32 = 1.0;
const DEFAULT_SPREAD: f32 = 1.5;

/// Newton steps per sample. Warm-started from the previous sample's solution,
/// two steps converge to well below f32 output resolution.
const NEWTON_ITERATIONS: usize = 2;

/// Tilt outputs span ±90° as ±5V.
const TILT_VOLTS_PER_RADIAN: f32 = 5.0 / FRAC_PI_2;

fn default_waves() -> usize {
    6
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct SwellParams {
    /// dominant swell frequency in V/Oct (use `$hz()` or a value like `'0.1hz'`)
    #[signal(type = pitch)]
    freq: PolySignal,
    /// crest sharpness: 0 rolls as a sine, 5 peaks into sharp crests over wide, flat troughs
    #[signal(type = control, default = 3.0, range = (0, 5))]
    #[deserr(default)]
    steepness: Option<PolySignal>,
    /// spectral width: the other waves sit up to `chop / 2` octaves above `freq`.
    /// 0 is one clean, periodic swell; higher values roll in sets and add chop
    #[signal(type = control, default = 1.0, range = (0, 5))]
    #[deserr(default)]
    chop: Option<PolySignal>,
    /// directional spread: 0 sends every wave the same way, 5 brings them from
    /// every direction (a confused sea)
    #[signal(type = control, default = 1.5, range = (0, 5))]
    #[deserr(default)]
    spread: Option<PolySignal>,
    /// number of wave components (1–16)
    #[serde(default = "default_waves")]
    #[deserr(default = default_waves())]
    waves: usize,
    /// picks the sea: each seed, and each channel, places the waves at
    /// different frequencies and directions
    #[serde(default)]
    #[deserr(default)]
    seed: u32,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SwellOutputs {
    #[output("height", "surface height at a fixed point, ±5V", default, range = (-5.0, 5.0))]
    height: PolyOutput,
    #[output(
        "tilt",
        "surface tilt along the swell's direction, ±5V = ±90°",
        range = (-5.0, 5.0)
    )]
    tilt: PolyOutput,
    #[output(
        "roll",
        "surface tilt across the swell's direction, ±5V = ±90°",
        range = (-5.0, 5.0)
    )]
    roll: PolyOutput,
}

/// One Gerstner wave component. Distances are in units where the dominant
/// wave's wavenumber is 1.
#[derive(Clone, Copy, Default)]
struct Wave {
    /// Phase in cycles, `[0, 1)`.
    phase: f32,
    /// Position in the spectrum, `[0, 1]`: 0 at `freq`, 1 at the top of the chop range.
    place: f32,
    /// Direction in `[-1, 1]`, scaled by `spread`.
    heading: f32,
    /// Frequency relative to `freq`.
    ratio: f32,
    /// Wavenumber; deep-water dispersion makes it `ratio²`.
    k: f32,
    /// Height amplitude, normalized so the amplitudes of all waves sum to 1.
    amp: f32,
    /// Orbit radius per unit of total steepness. `orbit · k` sums to 1 over
    /// the waves, so total steepness is the share every wave splits evenly.
    orbit: f32,
    /// Unit travel direction.
    dir: [f32; 2],
}

#[derive(Clone, Copy)]
struct ChannelState {
    waves: [Wave; MAX_WAVES],
    /// Rest position of the water particle currently at the gauge (the
    /// origin). Each sample's solve starts from the previous one.
    origin: [f32; 2],
    /// `(chop, spread)` the wave shapes were built for; NaN forces a rebuild.
    shaped_for: (f32, f32),
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            waves: [Wave::default(); MAX_WAVES],
            origin: [0.0; 2],
            shaped_for: (f32::NAN, f32::NAN),
        }
    }
}

/// Surface at the gauge: normalized height in `[-1, 1]` and the physical slope.
struct Surface {
    height: f32,
    slope: [f32; 2],
}

impl ChannelState {
    /// Place each wave in the spectrum and pick its heading. Wave 0 is the
    /// dominant swell: at `freq`, travelling along +x.
    fn scatter(&mut self, seed: u32, channel: usize) {
        let seed = u64::from(seed);
        for (i, wave) in self.waves.iter_mut().enumerate().skip(1) {
            let index = ((channel * MAX_WAVES + i) * 2) as u64;
            wave.place = (hash_bipolar(seed, index) * 0.5 + 0.5) as f32;
            wave.heading = hash_bipolar(seed, index + 1) as f32;
        }
        self.shaped_for = (f32::NAN, f32::NAN);
    }

    fn shape(&mut self, count: usize, chop: f32, spread: f32) {
        let octaves = chop.clamp(0.0, 5.0) * CHOP_OCTAVES_PER_VOLT;
        let span = spread.clamp(0.0, 5.0) * SPREAD_RADIANS_PER_VOLT;
        let waves = &mut self.waves[..count];
        // Every wave is equally steep, so height amplitude falls as 1/k.
        let mut amp_sum = 0.0;
        for wave in waves.iter_mut() {
            wave.ratio = (octaves * wave.place).exp2();
            wave.k = wave.ratio * wave.ratio;
            wave.orbit = 1.0 / (count as f32 * wave.k);
            amp_sum += wave.orbit;
            let (sin, cos) = (span * wave.heading).sin_cos();
            wave.dir = [cos, sin];
        }
        for wave in waves.iter_mut() {
            wave.amp = wave.orbit / amp_sum;
        }
    }

    /// Find the surface over the origin for total steepness `q`.
    ///
    /// A Gerstner wave moves each water particle in a circle around its rest
    /// position `p`, so the surface over a fixed point belongs to whichever
    /// particle is there now: solve `p − q·Σ orbit·dir·sin θ(p) = 0` by Newton.
    /// The Jacobian `J = I − q·Σ orbit·k·cos θ·dir·dirᵀ` is symmetric with
    /// eigenvalues at least `1 − q`, so it is always invertible.
    fn solve(&mut self, count: usize, q: f32) -> Surface {
        let waves = &self.waves[..count];
        let mut p = if self.origin.iter().all(|v| v.is_finite()) {
            self.origin
        } else {
            [0.0; 2]
        };
        let mut iteration = 0;
        loop {
            let mut offset = [0.0f32; 2];
            let mut fold = [0.0f32; 3];
            let mut rise = [0.0f32; 2];
            let mut height = 0.0;
            for wave in waves {
                let [dx, dy] = wave.dir;
                let theta = wave.k * (dx * p[0] + dy * p[1]) - TAU * wave.phase;
                let (sin, cos) = theta.sin_cos();
                let r = wave.orbit * sin;
                offset[0] += r * dx;
                offset[1] += r * dy;
                let bend = wave.orbit * wave.k * cos;
                fold[0] += bend * dx * dx;
                fold[1] += bend * dx * dy;
                fold[2] += bend * dy * dy;
                let lean = wave.orbit * wave.k * sin;
                rise[0] += lean * dx;
                rise[1] += lean * dy;
                height += wave.amp * cos;
            }
            let (jxx, jxy, jyy) = (1.0 - q * fold[0], -q * fold[1], 1.0 - q * fold[2]);
            let inv_det = 1.0 / (jxx * jyy - jxy * jxy);
            let (ixx, ixy, iyy) = (jyy * inv_det, -jxy * inv_det, jxx * inv_det);
            if iteration == NEWTON_ITERATIONS {
                self.origin = p;
                // Height gradient over rest positions, carried to surface
                // positions through J⁻¹.
                let (gx, gy) = (-q * rise[0], -q * rise[1]);
                return Surface {
                    height,
                    slope: [ixx * gx + ixy * gy, ixy * gx + iyy * gy],
                };
            }
            let (fx, fy) = (p[0] - q * offset[0], p[1] - q * offset[1]);
            p[0] -= ixx * fx + ixy * fy;
            p[1] -= ixy * fx + iyy * fy;
            iteration += 1;
        }
    }
}

/// Ocean swell as CV: the surface of a simulated sea, read at a fixed point.
///
/// The sea sums `waves` Gerstner waves. Each moves the water in circles rather
/// than just up and down, which bunches the surface into sharp crests between
/// wide, flat troughs — the shape of real swell, set by `steepness`. The first
/// wave rolls at `freq`; `chop` spreads the others above it, so their crests
/// drift in and out of line and the sea arrives in sets, with the occasional
/// wave far bigger than the rest. `spread` scatters their directions.
///
/// - **height** (default) — surface height, ±5V. It reaches ±5V only when
///   every wave lines up, so a sea with `chop` above 0 usually stays inside it.
/// - **tilt** / **roll** — slope of the surface along and across the swell's
///   direction, as the angle a moored boat would lean (±5V = ±90°). Tilt swings
///   hardest at each crest, flipping sign as it passes, more so as `steepness` rises.
///
/// Every input can be modulated live: the waves bend to the new values without
/// restarting. Each channel is its own sea.
///
/// ## Example
///
/// ```js
/// // A slow, steep swell sweeping a filter
/// $lpf($saw('a1'), $swell('0.1hz', { steepness: 4 }).range('200hz', '2000hz')).out()
///
/// // A choppy, confused sea: the surface leans as a boat would
/// const sea = $swell('0.2hz', { chop: 4, spread: 5, waves: 12 })
/// $sine('c3').amp(sea.tilt.range(0, 5)).out()
/// ```
#[module(name = "$swell", patch_update, args(freq))]
pub struct Swell {
    outputs: SwellOutputs,
    params: SwellParams,
    channel_state: Box<[ChannelState]>,
}

impl Swell {
    fn update(&mut self, sample_rate: f32) {
        let count = self.params.waves.clamp(1, MAX_WAVES);
        for ch in 0..self.channel_count() {
            let cycles = voct_to_hz(self.params.freq.get_value(ch)) / sample_rate;
            let q = (self.params.steepness.value_or(ch, DEFAULT_STEEPNESS) * STEEPNESS_PER_VOLT)
                .clamp(0.0, MAX_STEEPNESS);
            let chop = self.params.chop.value_or(ch, DEFAULT_CHOP);
            let spread = self.params.spread.value_or(ch, DEFAULT_SPREAD);

            let state = &mut self.channel_state[ch];
            if state.shaped_for != (chop, spread) {
                state.shape(count, chop, spread);
                state.shaped_for = (chop, spread);
            }
            for wave in &mut state.waves[..count] {
                wave.phase = wrap_phase(wave.phase + cycles * wave.ratio);
            }
            let surface = state.solve(count, q);

            self.outputs.height.set(ch, surface.height * 5.0);
            self.outputs
                .tilt
                .set(ch, surface.slope[0].atan() * TILT_VOLTS_PER_RADIAN);
            self.outputs
                .roll
                .set(ch, surface.slope[1].atan() * TILT_VOLTS_PER_RADIAN);
        }
    }
}

impl crate::types::PatchUpdateHandler for Swell {
    /// Re-scatters the waves from the current `seed`. Phases and the gauge
    /// solution carry over, so the sea keeps moving through a patch update.
    fn on_patch_update(&mut self) {
        let seed = self.params.seed;
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            state.scatter(seed, ch);
        }
    }
}

message_handlers!(impl Swell {});
