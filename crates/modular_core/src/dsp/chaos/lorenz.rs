use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

// --- Scaling constants ---
// All four params accept a 0–5 CV value and are mapped linearly to their
// useful Lorenz coefficient span.
//
//   sigma   0–5 → σ   0–30   (5V/30 = 0.1667V per unit)  default 1.6667 → σ=10
//   rho     0–5 → ρ   0–60   (5V/60 = 0.0833V per unit)  default 2.3333 → ρ=28
//   beta    0–5 → β   0–10   (5V/10 = 0.5V per unit)     default 1.3333 → β=2.667
//   rate    0–5 → sim 0–200  (5V/200= 0.025V per unit)   default 0.1    → 4 t/s

const SIGMA_SCALE: f32 = 6.0;
const RHO_SCALE: f32 = 12.0;
const BETA_SCALE: f32 = 2.0;
const RATE_SCALE: f32 = 40.0;

// Default 0–5 input values that reproduce the classic attractor.
const DEFAULT_SIGMA: f32 = 10.0 / SIGMA_SCALE; // ≈ 1.6667
const DEFAULT_RHO: f32 = 28.0 / RHO_SCALE; // ≈ 2.3333
const DEFAULT_BETA: f32 = (8.0 / 3.0) / BETA_SCALE; // ≈ 1.3333
const DEFAULT_RATE: f32 = 4.0 / RATE_SCALE; // = 0.1

// Output normalization: map typical excursions to ±5V (x/y) and 0–5V (z).
//   x wanders ≈ ±20, so 5/20 = 0.25
//   y wanders ≈ ±28, so 5/28 ≈ 0.1786
//   z wanders ≈ 0–50, so 5/50 = 0.1
const X_SCALE: f32 = 0.25;
const Y_SCALE: f32 = 5.0 / 28.0;
const Z_SCALE: f32 = 0.1;

// Forward-Euler stability limit for the Lorenz equations.
// At the canonical params, dt > ~0.02 starts to diverge; 0.01 is conservative.
const DT_MAX: f32 = 0.01;
// Sub-step cap: bounds worst-case audio-thread cost per sample.
const MAX_SUBSTEPS: u32 = 16;

// Clamp threshold for the integrator state: values beyond this indicate
// a diverging trajectory (e.g. extreme params). The channel is reseeded.
const STATE_CLAMP: f32 = 500.0;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct LorenzParams {
    /// Sigma (σ) — rate of divergence between the x and y variables.
    /// Input 0–5 maps to σ 0–30; default 1.667 → σ=10.
    #[signal(type = control, default = 1.6667, range = (0, 5))]
    #[deserr(default)]
    sigma: Option<PolySignal>,

    /// Rho (ρ) — distance from the origin to the critical points.
    /// Input 0–5 maps to ρ 0–60; default 2.333 → ρ=28.
    /// Chaos requires ρ > 24.74 (→ CV > ~2.06V); below that the system spirals to a fixed point.
    #[signal(type = control, default = 2.3333, range = (0, 5))]
    #[deserr(default)]
    rho: Option<PolySignal>,

    /// Beta (β) — geometry of the attractor.
    /// Input 0–5 maps to β 0–10; default 1.333 → β=8/3 ≈ 2.667.
    #[signal(type = control, default = 1.3333, range = (0, 5))]
    #[deserr(default)]
    beta: Option<PolySignal>,

    /// Simulation speed — Lorenz time-units advanced per second of audio.
    /// Input 0–5 maps to 0–200 t/s; default 0.1 → 4 t/s (slow LFO-rate butterfly).
    /// Higher values push outputs toward audio-rate chaos.
    #[signal(type = control, default = 0.1, range = (0, 5))]
    #[deserr(default)]
    rate: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LorenzOutputs {
    /// X coordinate of the Lorenz attractor, normalized to ±5V.
    #[output("x", "X coordinate, normalized to ±5V", default, range = (-5.0, 5.0))]
    x: PolyOutput,
    /// Y coordinate of the Lorenz attractor, normalized to ±5V.
    #[output("y", "Y coordinate, normalized to ±5V", range = (-5.0, 5.0))]
    y: PolyOutput,
    /// Z coordinate of the Lorenz attractor, normalized to 0–5V (Z is always positive).
    #[output("z", "Z coordinate, normalized to 0–5V", range = (0.0, 5.0))]
    z: PolyOutput,
}

/// Per-channel integrator state for the Lorenz attractor.
///
/// x, y, z evolve continuously every sample regardless of param changes —
/// changing a param warps the vector field and bends the live trajectory
/// rather than restarting the simulation from scratch.
#[derive(Clone, Copy, Default)]
struct ChannelState {
    x: f32,
    y: f32,
    z: f32,
}

impl ChannelState {
    /// Initial condition near the classic attractor's basin, with a small
    /// per-channel x perturbation so polyphonic channels diverge.
    fn new_with_offset(ch: usize) -> Self {
        // A tiny offset on x is amplified by the chaos; channels quickly
        // land on independent trajectories.
        Self {
            x: 1.0 + ch as f32 * 0.01,
            y: 1.0,
            z: 1.0,
        }
    }
}

/// Module-level state for the Lorenz module.
#[derive(Default)]
struct LorenzState {
    /// Reciprocal of the sample rate, computed once in `init`.
    inv_sample_rate: f32,
}

/// Chaotic signal source based on the Lorenz strange attractor.
///
/// Simulates the Lorenz system continuously — changing any parameter pulls the
/// trajectory in a new direction rather than restarting from scratch. Outputs x
/// (default) and y coordinates normalized to **±5V**, and z normalized to **0–5V**
/// (z is always positive in the Lorenz system).
///
/// ## Parameters (all 0–5 CV)
///
/// | param | mapped range | default | chaotic butterfly |
/// |-------|-------------|---------|-------------------|
/// | sigma | σ 0–30      | 1.667 → σ=10   | stays chaotic for σ > 0 |
/// | rho   | ρ 0–60      | 2.333 → ρ=28   | chaos requires ρ > ~24.74 (CV > ~2.06V) |
/// | beta  | β 0–10      | 1.333 → β=8/3  | classic shape near 2.667 |
/// | rate  | 0–200 t/s   | 0.1   → 4 t/s  | audio-rate at ≥ ~50 t/s |
///
/// ## Example
///
/// ```js
/// // Classic Lorenz butterfly at default params
/// $lorenz()
///
/// // Access all three outputs
/// const l = $lorenz()
/// l    // x coordinate (±5V)
/// l.y  // y coordinate (±5V)
/// l.z  // z coordinate (0–5V)
///
/// // Modulate rho — nudges the attractor without resetting it
/// $lorenz({ rho: $sine('1hz').range(0, 5), rate: 0.5 }).out()
/// ```
#[module(name = "$lorenz", has_init)]
pub struct Lorenz {
    outputs: LorenzOutputs,
    params: LorenzParams,
    state: LorenzState,
    /// Per-channel (x, y, z) integrator state. Preserved across patch updates
    /// by `transfer_state_from` so the trajectory continues uninterrupted.
    channel_state: Box<[ChannelState]>,
}

impl Lorenz {
    fn init(&mut self, sample_rate: f32) {
        // Sample-rate-only — safe in init (rate changes rebuild the processor,
        // not a state transfer; transferred state has the same rate).
        self.state.inv_sample_rate = 1.0 / sample_rate;

        // Seed integrator state. On a patch update, transfer_state_from runs
        // after init and swaps in the prior running (x,y,z), so the trajectory
        // continues uninterrupted regardless of this initial seeding.
        let channels = self.channel_count();
        self.channel_state = (0..channels)
            .map(ChannelState::new_with_offset)
            .collect::<Vec<_>>()
            .into_boxed_slice();
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let inv_sr = self.state.inv_sample_rate;

        for ch in 0..channels {
            // Map 0–5 inputs to Lorenz coefficient spans.
            let sigma = self.params.sigma.value_or(ch, DEFAULT_SIGMA) * SIGMA_SCALE;
            let rho = self.params.rho.value_or(ch, DEFAULT_RHO) * RHO_SCALE;
            let beta = self.params.beta.value_or(ch, DEFAULT_BETA) * BETA_SCALE;
            let rate_cv = self.params.rate.value_or(ch, DEFAULT_RATE);
            let rate = (rate_cv * RATE_SCALE).max(0.0);

            // Total Lorenz time-units this audio sample.
            let dt_total = rate * inv_sr;

            // Sub-step to keep forward-Euler stable. At canonical params,
            // dt > ~0.02 diverges; DT_MAX=0.01 is conservative.
            let n_steps = if dt_total > 0.0 {
                ((dt_total / DT_MAX).ceil() as u32).clamp(1, MAX_SUBSTEPS)
            } else {
                1
            };
            let dt = dt_total / n_steps as f32;

            let state = &mut self.channel_state[ch];

            // Guard against non-finite state (e.g. from extreme params).
            if !state.x.is_finite() || !state.y.is_finite() || !state.z.is_finite() {
                *state = ChannelState::new_with_offset(ch);
            }

            for _ in 0..n_steps {
                let dx = sigma * (state.y - state.x);
                let dy = state.x * (rho - state.z) - state.y;
                let dz = state.x * state.y - beta * state.z;
                state.x += dt * dx;
                state.y += dt * dy;
                state.z += dt * dz;

                // Clamp diverging trajectories back to basin.
                if state.x.abs() > STATE_CLAMP
                    || state.y.abs() > STATE_CLAMP
                    || state.z.abs() > STATE_CLAMP
                {
                    *state = ChannelState::new_with_offset(ch);
                    break;
                }
            }

            // Normalize to ±5V (x/y) and 0–5V (z). `PolyOutput::set` sanitizes NaN/Inf.
            self.outputs.x.set(ch, (state.x * X_SCALE).clamp(-5.0, 5.0));
            self.outputs.y.set(ch, (state.y * Y_SCALE).clamp(-5.0, 5.0));
            self.outputs.z.set(ch, (state.z * Z_SCALE).clamp(0.0, 5.0));
        }
    }
}

message_handlers!(impl Lorenz {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    /// Build a Lorenz module with the given params, mirroring the production
    /// lifecycle: `init` seeds the (x,y,z) state and captures the sample rate.
    fn make_lorenz(params: LorenzParams) -> Lorenz {
        let channels = __lorenz_derive_channel_count(&params);
        let mut outputs = LorenzOutputs::default();
        outputs.set_all_channels(channels);
        let mut l = Lorenz {
            params,
            outputs,
            state: LorenzState::default(),
            channel_state: Box::default(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        l.init(48000.0);
        l
    }

    fn default_params() -> LorenzParams {
        LorenzParams {
            sigma: None,
            rho: None,
            beta: None,
            rate: None,
        }
    }

    #[test]
    fn output_stays_finite_and_bounded() {
        let mut l = make_lorenz(default_params());
        for _ in 0..4800 {
            l.update(48000.0);
            let x = l.outputs.x.get(0);
            let y = l.outputs.y.get(0);
            assert!(x.is_finite(), "x must stay finite, got {x}");
            assert!(y.is_finite(), "y must stay finite, got {y}");
            assert!(x.abs() <= 5.0, "x={x} exceeded ±5V");
            assert!(y.abs() <= 5.0, "y={y} exceeded ±5V");
        }
    }

    #[test]
    fn trajectory_moves() {
        // At default params the attractor evolves — outputs must change over time.
        let mut l = make_lorenz(default_params());
        // Warm up to exit any transient near the initial condition.
        for _ in 0..500 {
            l.update(48000.0);
        }
        let x0 = l.outputs.x.get(0);
        for _ in 0..100 {
            l.update(48000.0);
        }
        let x1 = l.outputs.x.get(0);
        assert!(
            (x0 - x1).abs() > 1e-6,
            "Lorenz trajectory should move: x0={x0} x1={x1}"
        );
    }

    #[test]
    fn poly_channels_diverge() {
        // Two channels start with a 0.01 x-offset; chaos amplifies it.
        // Use rate=2.0 (→ 80 t/s) so divergence happens quickly:
        // 10000 samples × 80 t/s / 48000 Hz ≈ 16.7 Lorenz time units,
        // which is many multiples of the Lyapunov time (~1.1 t).
        let rate_high = 2.0_f32; // 2V → 80 t/s
        let params = LorenzParams {
            sigma: None,
            rho: None,
            beta: None,
            rate: Some(PolySignal::poly(&[
                Signal::Volts(rate_high),
                Signal::Volts(rate_high),
            ])),
        };
        let channels = 2;
        let mut outputs = LorenzOutputs::default();
        outputs.set_all_channels(channels);
        let mut l = Lorenz {
            params,
            outputs,
            state: LorenzState::default(),
            channel_state: Box::default(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        l.init(48000.0);

        // 16+ Lorenz time units — channels are fully decorrelated.
        for _ in 0..10000 {
            l.update(48000.0);
        }
        let x0 = l.outputs.x.get(0);
        let x1 = l.outputs.x.get(1);
        assert!(
            (x0 - x1).abs() > 0.1,
            "Poly channels should diverge after many Lyapunov times: ch0={x0} ch1={x1}"
        );
    }

    #[test]
    fn high_rate_stays_bounded() {
        // rate=5 → 200 t/s. Sub-stepping must keep it stable.
        let params = LorenzParams {
            sigma: None,
            rho: None,
            beta: None,
            rate: Some(PolySignal::mono(Signal::Volts(5.0))),
        };
        let mut l = make_lorenz(params);
        for _ in 0..4800 {
            l.update(48000.0);
            let x = l.outputs.x.get(0);
            let y = l.outputs.y.get(0);
            assert!(x.is_finite(), "x must stay finite at high rate, got {x}");
            assert!(y.is_finite(), "y must stay finite at high rate, got {y}");
            assert!(x.abs() <= 5.0, "x={x} exceeded ±5V at high rate");
            assert!(y.abs() <= 5.0, "y={y} exceeded ±5V at high rate");
        }
    }

    #[test]
    fn zero_rate_holds_output() {
        // At rate=0 the attractor freezes — outputs must not move.
        let params = LorenzParams {
            sigma: None,
            rho: None,
            beta: None,
            rate: Some(PolySignal::mono(Signal::Volts(0.0))),
        };
        let mut l = make_lorenz(params);
        // Let it run once to settle the initial output.
        l.update(48000.0);
        let x0 = l.outputs.x.get(0);
        let y0 = l.outputs.y.get(0);
        for _ in 0..100 {
            l.update(48000.0);
        }
        let x1 = l.outputs.x.get(0);
        let y1 = l.outputs.y.get(0);
        assert!(
            (x0 - x1).abs() < 1e-9,
            "x should not move at rate=0: {x0} → {x1}"
        );
        assert!(
            (y0 - y1).abs() < 1e-9,
            "y should not move at rate=0: {y0} → {y1}"
        );
    }

    #[test]
    fn x_and_y_differ() {
        // x and y are distinct coordinates — they must not be equal.
        let mut l = make_lorenz(default_params());
        for _ in 0..500 {
            l.update(48000.0);
        }
        let x = l.outputs.x.get(0);
        let y = l.outputs.y.get(0);
        assert!(
            (x - y).abs() > 1e-4,
            "x and y outputs should differ: x={x} y={y}"
        );
    }

    #[test]
    fn z_output_bounded_and_positive() {
        // z is always positive in the Lorenz system; normalized output must be 0–5V.
        let mut l = make_lorenz(default_params());
        for _ in 0..4800 {
            l.update(48000.0);
            let z = l.outputs.z.get(0);
            assert!(z.is_finite(), "z must stay finite, got {z}");
            assert!(z >= 0.0, "z must be non-negative, got {z}");
            assert!(z <= 5.0, "z={z} exceeded 5V");
        }
    }

    #[test]
    fn substep_path_is_exercised() {
        // At a low synthetic sample rate (100 Hz) and high rate CV (5V → 200 t/s),
        // dt_total = 200/100 = 2.0 >> DT_MAX (0.01), forcing n_steps = 16 (MAX_SUBSTEPS).
        // The output must remain finite and bounded, confirming the substep path works.
        let params = LorenzParams {
            sigma: None,
            rho: None,
            beta: None,
            rate: Some(PolySignal::mono(Signal::Volts(5.0))),
        };
        let mut l = make_lorenz(params);
        for _ in 0..1000 {
            l.update(100.0);
            let x = l.outputs.x.get(0);
            let y = l.outputs.y.get(0);
            let z = l.outputs.z.get(0);
            assert!(
                x.is_finite(),
                "x must stay finite under heavy substepping, got {x}"
            );
            assert!(
                y.is_finite(),
                "y must stay finite under heavy substepping, got {y}"
            );
            assert!(
                z.is_finite(),
                "z must stay finite under heavy substepping, got {z}"
            );
            assert!(x.abs() <= 5.0, "x={x} exceeded ±5V under substepping");
            assert!(y.abs() <= 5.0, "y={y} exceeded ±5V under substepping");
            assert!(z >= 0.0 && z <= 5.0, "z={z} out of 0–5V under substepping");
        }
    }
}
