use std::f32::consts::{PI, TAU};

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::{rng::hash_bipolar, voct_to_hz, wrap_phase};
use crate::param_errors::ModuleParamErrors;
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

/// Gauge-solve residual, in units of the dominant wavelength over 2π, at
/// which the surface position counts as found.
const TOLERANCE: f32 = 1e-5;
/// Newton steps a sample may take. Warm-started from the previous sample, the
/// solve usually needs one or two.
const MAX_ITERATIONS: usize = 8;

/// Keeps `hash_bipolar`'s seed half disjoint from its index half, so no seed
/// reproduces another seed's (or channel's) waves.
const SEED_SALT: u64 = 0x5EED;

fn default_waves() -> usize {
    6
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(
    rename_all = camelCase,
    deny_unknown_fields,
    validate = validate_swell_params -> ModuleParamErrors
)]
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
    /// number of wave components (1–16); waves added or removed by a patch
    /// update fade over one swell period
    #[serde(default = "default_waves")]
    #[deserr(default = default_waves())]
    #[schemars(range(min = 1, max = 16))]
    waves: usize,
    /// picks the sea: each seed, and each channel, places the waves at
    /// different frequencies and directions
    #[serde(default)]
    #[deserr(default)]
    seed: u32,
}

fn validate_swell_params(
    params: SwellParams,
    _location: deserr::ValuePointerRef,
) -> Result<SwellParams, ModuleParamErrors> {
    if !(1..=MAX_WAVES).contains(&params.waves) {
        let mut err = ModuleParamErrors::default();
        err.add(
            "waves".to_string(),
            format!("must be between 1 and {MAX_WAVES}, got {}", params.waves),
        );
        return Err(err);
    }
    Ok(params)
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SwellOutputs {
    #[output("height", "surface height at a fixed point, ±5V", default, range = (-5.0, 5.0))]
    height: PolyOutput,
    #[output(
        "tilt",
        "lean along the swell: positive while the water rises, ±5V",
        range = (-5.0, 5.0)
    )]
    tilt: PolyOutput,
    #[output(
        "roll",
        "lean across the swell from waves arriving off-axis, ±5V",
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
    /// Fade level in `[0, 1]`; 0 is silent.
    presence: f32,
    /// Frequency relative to `freq`.
    ratio: f32,
    /// Wavenumber; deep-water dispersion makes it `ratio²`.
    k: f32,
    /// Orbit radius per unit of total steepness, which is also the wave's
    /// height amplitude. `orbit · k` sums to 1 over the waves.
    orbit: f32,
    /// Unit travel direction.
    dir: [f32; 2],
}

#[derive(Clone, Copy)]
struct ChannelState {
    waves: [Wave; MAX_WAVES],
    /// Wave slots in play: the `waves` param plus any still fading out.
    span: usize,
    /// False until the first patch update brings the waves in.
    started: bool,
    /// Rest position of the water particle currently at the gauge (the
    /// origin). Each sample's solve starts from the previous one.
    origin: [f32; 2],
    /// `chop` the ratios were tuned for; NaN forces a retune.
    tuned_for: f32,
    /// `spread` the directions were aimed for; NaN forces a re-aim.
    aimed_for: f32,
    /// Reciprocal of the summed orbits, normalizing height to `[-1, 1]`.
    height_scale: f32,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            waves: [Wave::default(); MAX_WAVES],
            span: 0,
            started: false,
            origin: [0.0; 2],
            tuned_for: f32::NAN,
            aimed_for: f32::NAN,
            height_scale: 0.0,
        }
    }
}

/// Surface at the gauge: normalized height in `[-1, 1]` and the slope per
/// unit of steepness.
struct Surface {
    height: f32,
    lean: [f32; 2],
}

impl ChannelState {
    /// Place each wave in the spectrum and pick its heading. Wave 0 is the
    /// dominant swell: at `freq`, travelling along +x.
    ///
    /// A new channel brings its waves in at full strength with every crest
    /// aligned, so the sea starts on a full swell. Waves a later update adds
    /// start at scattered phases and fade in, so they do not arrive as one
    /// extra crest.
    fn scatter(&mut self, seed: u32, channel: usize, count: usize) {
        let salted = (u64::from(seed) << 32) | SEED_SALT;
        for (i, wave) in self.waves.iter_mut().enumerate() {
            let index = ((channel * MAX_WAVES + i) * 3) as u64;
            if i > 0 {
                wave.place = (hash_bipolar(salted, index) * 0.5 + 0.5) as f32;
                wave.heading = hash_bipolar(salted, index + 1) as f32;
            }
            if !self.started && i < count {
                wave.presence = 1.0;
            } else if wave.presence == 0.0 {
                wave.phase = (hash_bipolar(salted, index + 2) * 0.5 + 0.5) as f32;
            }
        }
        self.started = true;
        self.span = self.span.max(count);
        self.tuned_for = f32::NAN;
        self.aimed_for = f32::NAN;
    }

    /// Advance one sample: follow `chop` and `spread`, fade waves toward
    /// `count`, and move every phase on by `cycles` of the dominant swell.
    fn advance(&mut self, count: usize, cycles: f32, chop: f32, spread: f32) {
        let mut reweigh = false;
        if self.tuned_for != chop {
            self.tuned_for = chop;
            let octaves = chop.clamp(0.0, 5.0) * CHOP_OCTAVES_PER_VOLT;
            for wave in &mut self.waves[..self.span] {
                wave.ratio = (octaves * wave.place).exp2();
                wave.k = wave.ratio * wave.ratio;
            }
            reweigh = true;
        }
        if self.aimed_for != spread {
            self.aimed_for = spread;
            let span = spread.clamp(0.0, 5.0) * SPREAD_RADIANS_PER_VOLT;
            for wave in &mut self.waves[..self.span] {
                let (sin, cos) = (span * wave.heading).sin_cos();
                wave.dir = [cos, sin];
            }
        }
        for (i, wave) in self.waves[..self.span].iter_mut().enumerate() {
            if i < count && wave.presence < 1.0 {
                wave.presence = (wave.presence + cycles).min(1.0);
                reweigh = true;
            } else if i >= count && wave.presence > 0.0 {
                wave.presence = (wave.presence - cycles).max(0.0);
                reweigh = true;
            }
            wave.phase = wrap_phase(wave.phase + cycles * wave.ratio);
        }
        while self.span > count && self.waves[self.span - 1].presence == 0.0 {
            self.span -= 1;
        }
        if reweigh {
            self.weigh();
        }
    }

    /// Share the steepness among the waves by presence. Each wave is then
    /// equally steep for its presence, so its orbit and height fall as 1/k.
    fn weigh(&mut self) {
        let waves = &mut self.waves[..self.span];
        let total: f32 = waves.iter().map(|w| w.presence).sum();
        if total <= 0.0 {
            waves.iter_mut().for_each(|w| w.orbit = 0.0);
            self.height_scale = 0.0;
            return;
        }
        let mut orbit_sum = 0.0;
        for wave in waves {
            wave.orbit = wave.presence / (total * wave.k);
            orbit_sum += wave.orbit;
        }
        self.height_scale = 1.0 / orbit_sum;
    }

    /// Find the surface over the origin for total steepness `q`.
    ///
    /// A Gerstner wave moves each water particle in a circle around its rest
    /// position `p`, so the surface over a fixed point belongs to whichever
    /// particle is there now: solve `p − q·Σ orbit·dir·sin θ(p) = 0` by Newton.
    /// The Jacobian `J = I − q·Σ orbit·k·cos θ·dir·dirᵀ` is symmetric with
    /// eigenvalues at least `1 − q`, so it is always invertible.
    fn solve(&mut self, q: f32) -> Surface {
        let waves = &self.waves[..self.span];
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
                height += wave.orbit * cos;
            }
            let (jxx, jxy, jyy) = (1.0 - q * fold[0], -q * fold[1], 1.0 - q * fold[2]);
            let inv_det = 1.0 / (jxx * jyy - jxy * jxy);
            let (ixx, ixy, iyy) = (jyy * inv_det, -jxy * inv_det, jxx * inv_det);
            let (fx, fy) = (p[0] - q * offset[0], p[1] - q * offset[1]);
            if fx.hypot(fy) < TOLERANCE || iteration == MAX_ITERATIONS {
                self.origin = p;
                // Height gradient over rest positions, carried to surface
                // positions through J⁻¹.
                let (gx, gy) = (-rise[0], -rise[1]);
                return Surface {
                    height: height * self.height_scale,
                    lean: [ixx * gx + ixy * gy, ixy * gx + iyy * gy],
                };
            }
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
/// - **tilt** — how the surface leans along the swell, the way a moored boat
///   pitches: positive while the water rises, negative while it falls. A sine
///   at `steepness` 0; when steep, a sawtooth that snaps down at each crest.
/// - **roll** — the sideways lean from waves arriving off-axis. Silent at
///   `spread` 0, and busier as `spread` and `chop` rise.
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
/// // A choppy, confused sea: the swell's lean swaying a sine's level
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
        let count = self.params.waves;
        for ch in 0..self.channel_count() {
            let cycles = voct_to_hz(self.params.freq.get_value(ch)) / sample_rate;
            let q = (self.params.steepness.value_or(ch, DEFAULT_STEEPNESS) * STEEPNESS_PER_VOLT)
                .clamp(0.0, MAX_STEEPNESS);
            let chop = self.params.chop.value_or(ch, DEFAULT_CHOP);
            let spread = self.params.spread.value_or(ch, DEFAULT_SPREAD);

            let state = &mut self.channel_state[ch];
            state.advance(count, cycles, chop, spread);
            let surface = state.solve(q);

            // A single wave's slope peaks at 1/√(1 − q²); scaling by its
            // reciprocal keeps the lean near ±5V at every steepness.
            let lean_volts = -5.0 * (1.0 - q * q).sqrt();
            self.outputs.height.set(ch, surface.height * 5.0);
            self.outputs
                .tilt
                .set(ch, (surface.lean[0] * lean_volts).clamp(-5.0, 5.0));
            self.outputs
                .roll
                .set(ch, (surface.lean[1] * lean_volts).clamp(-5.0, 5.0));
        }
    }
}

impl crate::types::PatchUpdateHandler for Swell {
    /// Re-scatters the waves from the current `seed`. Phases and the gauge
    /// solution carry over, so the sea keeps moving through a patch update.
    fn on_patch_update(&mut self) {
        let (seed, count) = (self.params.seed, self.params.waves);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            state.scatter(seed, ch, count);
        }
    }
}

message_handlers!(impl Swell {});
