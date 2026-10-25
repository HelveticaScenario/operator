//! A group of four channels: its state, and the signal path run one sample at a
//! time or over a block.

use std::ops::Range;

use crate::dsp::utils::{rng::LcgRng, sanitize};

use super::components::{self, ZENER_PAIR_CAPACITANCE_0V, ZenerTables};
use super::controls::{ControlInputs, Controls, row, update_controls};
use super::curve::Curve;
use super::drive::{self, DriveCurves, DriveState, PreEmphasis};
use super::lanes::{HalfbandDown, HalfbandUp, LANES, Lanes, OnePole, f32x4};
use super::output::{self, OUTPUT_POLE_HZ, output_amp, rails};
use super::svf::{NEGATIVE_DAMPING, SvfCore};
use super::vca::{self, GainX4};
use super::{MultimodeBlockOutputs, MultimodeParamsBlockInputs};

/// Mode VCA input resistors (R96, R98, and the R111/R118 summer's equivalent).
const R_MODE_IN: f32 = 62.0e3;
/// Drive VCA input resistor (R33).
const R_DRIVE_IN: f32 = 30.0e3;
/// Drive transimpedance feedback: R24 (30k) across C12 (18 pF) and the D4/D5
/// zener pair at its 0 V capacitance.
const DRIVE_POLE_HZ: f32 =
    1.0 / (std::f32::consts::TAU * 30.0e3 * (18.0e-12 + ZENER_PAIR_CAPACITANCE_0V));
/// Circuit noise at the filter's summing node (~10 µV). Self-oscillation grows
/// from it, so the filter starts ringing without any input, as the hardware does.
const NOISE_FLOOR_V: f32 = 1.0e-5;

/// The shared tables, fetched once per update so evaluations skip their
/// lazy-initialisation checks.
#[derive(Clone, Copy)]
pub(super) struct Tables {
    zener: &'static ZenerTables,
    drive: &'static DriveCurves,
    chain: &'static Curve,
}

impl Tables {
    pub(super) fn get() -> Self {
        Self {
            zener: components::zener_tables(),
            drive: drive::curves(),
            chain: output::chain(),
        }
    }
}

/// Four channels' state, one lane each; group `g` holds channels `4g..4g + 4`
/// and lives in `channel_state[g]`. With `highRes` the whole circuit runs at
/// twice the engine rate, and the four outputs are decimated separately.
#[derive(Default, Clone, Copy)]
pub(super) struct GroupState {
    /// Rate the core and its caches are configured for; 0 until configured.
    pub(super) core_rate: f32,
    pub(super) controls: Controls,
    pub(super) up: HalfbandUp,
    pub(super) down: [HalfbandDown; 4],
    pub(super) drive_pole: OnePole,
    pub(super) output_poles: [OnePole; 3],
    pub(super) drive: DriveState,
    pub(super) core: SvfCore,
    pub(super) noise: [LcgRng; LANES],
    pub(super) pre_emphasis: PreEmphasis,
}

impl GroupState {
    /// Sets the core's rate-dependent coefficients, clearing the oversampling
    /// filters and integrator caches when the rate changes.
    pub(super) fn configure(&mut self, rate: f32) {
        if self.core_rate == rate {
            return;
        }
        self.core_rate = rate;
        self.controls.tuning.clear();
        self.up = HalfbandUp::default();
        self.down = Default::default();
        self.pre_emphasis = PreEmphasis::default();
        let coeff = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / rate).exp();
        self.drive_pole.set_coeff(coeff(DRIVE_POLE_HZ));
        for pole in &mut self.output_poles {
            pole.set_coeff(coeff(OUTPUT_POLE_HZ));
        }
    }

    /// Returns lane `lane` to rest, as a fresh channel starts. Its noise
    /// generator keeps its seed.
    pub(super) fn clear_lane(&mut self, lane: usize) {
        let c = &mut self.controls;
        c.drive.clear_lane(lane);
        c.fold.clear_lane(lane);
        c.tuning.clear_lane(lane);
        c.res.clear_lane(lane);
        c.mode.clear_lane(lane);
        self.up.clear_lane(lane);
        self.down.iter_mut().for_each(|d| d.clear_lane(lane));
        self.drive_pole.clear_lane(lane);
        self.output_poles
            .iter_mut()
            .for_each(|p| p.clear_lane(lane));
        self.drive.clear_lane(lane);
        self.core.clear_lane(lane);
        self.pre_emphasis.clear_lane(lane);
    }

    /// One core-rate sample from drive VCA outputs `x`, returning the outputs
    /// `[output, lp, bp, hp]`. The output amp's feedback pole filters the LP,
    /// BP and HP nodes before the mode VCAs, which only rescale them, so
    /// filtering each one equals filtering their mix. The drive stage runs on
    /// the first `active` lanes.
    #[inline]
    fn tick<L: Lanes>(
        &mut self,
        x: L,
        fold_gains: &[(f32, f32); LANES],
        tables: Tables,
        active: usize,
    ) -> [L; 4] {
        let x = self.drive_pole.process(x);
        let filter_in = self.drive.process(tables.drive, x, fold_gains, active);
        let mut noise = [0.0; LANES];
        for (noise, rng) in noise.iter_mut().zip(&mut self.noise).take(active) {
            *noise = rng.next_bipolar();
        }
        let u = L::splat(NOISE_FLOOR_V) * L::from_lanes(noise) - filter_in;
        let c = &self.controls;
        let (lp, bp) = self
            .core
            .step(u, &c.tuning_x4, &c.res_x4, tables.zener, active);
        let hp = -filter_in - lp - L::splat(NEGATIVE_DAMPING) * bp;
        let [p_lp, p_bp, p_hp] = &mut self.output_poles;
        let (lp, bp, hp) = (p_lp.process(lp), p_bp.process(bp), p_hp.process(hp));
        let amp = |v| output_amp(tables.chain, v, active);
        [
            amp(morph(lp, bp, hp, &c.mode_x4)),
            amp(lp),
            amp(-bp),
            amp(hp),
        ]
    }
}

/// The mode VCAs' mix into the output amp, in volts across R173. VCA A takes
/// BP and HP through 62k each; VCA B takes its output less BP and LP (through
/// the IC14C summer) and adds to the LP that reaches the output directly.
#[inline]
fn morph<L: Lanes>(lp: L, bp: L, hp: L, (gain_a, gain_b): &(GainX4, GainX4)) -> L {
    // Below mode 2.5 VCA A is off, and at mode 0 VCA B too, so LP reaches the
    // output alone.
    let live = |gain: &GainX4| L::bits(gain.gain::<L>().ge(L::splat(f32::MIN_POSITIVE))) != 0;
    let (r, inv_r) = (L::splat(R_MODE_IN), L::splat(1.0 / R_MODE_IN));
    let a = if live(gain_a) {
        r * vca::output(gain_a, (bp + hp) * inv_r)
    } else {
        L::splat(0.0)
    };
    if !live(gain_b) {
        return lp;
    }
    let w = a - bp - lp;
    lp + r * vca::output(gain_b, w * inv_r)
}

/// One sample's `[output, lp, bp, hp]` for a group's lanes.
pub(super) type GroupOutputs = [[f32; LANES]; 4];

/// One sample for the `active` channels of group `state`, kept out of line so
/// `update` carries one inlined copy.
#[inline(never)]
pub(super) fn process_group<L: Lanes>(
    state: &mut GroupState,
    controls: &ControlInputs,
    input: [f32; LANES],
    (active, high_res, rate, tables): (usize, bool, f32, Tables),
) -> GroupOutputs {
    update_controls::<L>(&mut state.controls, controls, active, rate);
    step::<L>(state, input, active, high_res, tables)
}

/// Slots `range` for the `active` channels of group `state`, from channel
/// `base`, read from `inputs` and written to `outputs`. Controls held over the
/// range update once.
#[inline(never)]
pub(super) fn process_block_group<L: Lanes>(
    state: &mut GroupState,
    inputs: &MultimodeParamsBlockInputs,
    outputs: &mut MultimodeBlockOutputs,
    range: Range<usize>,
    (base, active, high_res, rate, tables): (usize, usize, bool, f32, Tables),
) {
    let held = [
        &inputs.cutoff,
        &inputs.resonance,
        &inputs.mode,
        &inputs.drive,
        &inputs.fold,
    ]
    .iter()
    .all(|input| input.is_constant());
    if held {
        let controls = ControlInputs::row(inputs, range.start, base, active);
        update_controls::<L>(&mut state.controls, &controls, active, rate);
    }
    for i in range {
        if !held {
            let controls = ControlInputs::row(inputs, i, base, active);
            update_controls::<L>(&mut state.controls, &controls, active, rate);
        }
        let input = row(&inputs.input, i, base, active);
        let [sample, lp, bp, hp] = step::<L>(state, input, active, high_res, tables);
        let ports = [
            (&mut outputs.sample, sample),
            (&mut outputs.lp, lp),
            (&mut outputs.bp, bp),
            (&mut outputs.hp, hp),
        ];
        for (port, values) in ports {
            let slot = &mut port.slot_mut(i)[base..base + active];
            for (out, &v) in slot.iter_mut().zip(&values) {
                *out = sanitize(v);
            }
        }
    }
}

/// One sample of the signal path for the first `active` lanes of group
/// `state`, returning each output's lanes.
#[inline(always)]
pub(super) fn step<L: Lanes>(
    state: &mut GroupState,
    input: [f32; LANES],
    active: usize,
    high_res: bool,
    tables: Tables,
) -> GroupOutputs {
    let c = &state.controls;
    let fold = c.fold_gains;
    let input = L::from_lanes(input);
    let drive_in = input * L::splat(1.0 / R_DRIVE_IN);
    let x = (L::splat(R_DRIVE_IN) * vca::output(&c.drive_x4, drive_in)).sanitize();
    let out = if high_res {
        let (even, odd) = state.up.process(x);
        let even = state.tick(even, &fold, tables, active);
        let odd = state.tick(odd, &fold, tables, active);
        // A lone channel decimates its four outputs as one downsampler's lanes.
        if L::WIDTH == 1 {
            let pack = |o: [L; 4]| f32x4::new(o.map(|v| v.to_lanes()[0]));
            let y = state.down[0].process(pack(even), pack(odd)).to_array();
            y.map(L::splat)
        } else {
            std::array::from_fn(|k| state.down[k].process(even[k], odd[k]))
        }
    } else {
        let x = state.pre_emphasis.process(x);
        state.tick(x, &fold, tables, active)
    };

    out.map(|v| rails(v).to_lanes())
}
