//! TR-808 cowbell, simulated from the 808 service notes' voicing board
//! schematic. Part names (R28, C9, …) are the service notes' designators.
//!
//! Two of the six Schmitt-trigger oscillators shared by the 808's metal
//! voices, tuned near 540 and 800 Hz, switch Q15 and Q14. Each transistor
//! pulls its node down through a diode while its oscillator is high; while
//! it is low the node rises toward the envelope on C9 through R28 or R29.
//! These "swing-type" VCAs feed IC2's band-pass filter. C9 is charged by the
//! trigger pulse and drains through the VCAs, and R82 with C34 across it
//! pulls the envelope down quickly at first, for a sharp attack.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{PULSE_SECS, pulse_volts};
use super::levels;
use super::metal::{COWBELL_HZ, DUTY, metal_pulse};
use crate::dsp::drums::analog::{high_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::input::{Cached, Hold, Input, Live, each_slot};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Timing, Voices, rails};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal};

mod module;

pub use module::Cb808;

// Network unknowns: the envelope on C9, the R82/C34 junction, the two VCA
// nodes, the junctions of R26/C30 and R27/C31, IC2's summing node M, its
// inverting input, its output, and its output current.
const ENV: usize = 0;
const C34_NODE: usize = 1;
const VCA_A: usize = 2;
const VCA_B: usize = 3;
const IN_A: usize = 4;
const IN_B: usize = 5;
const M: usize = 6;
const INV: usize = 7;
const OUT: usize = 8;
const OPAMP: usize = 9;
const UNKNOWNS: usize = 10;
const CAPS: usize = 6;
// Switches: the trigger pulse charging C9 through Q5, R122 and D2; Q15 and
// Q14 pulling their nodes down through D3 and D4 and R31/R33.
const CHARGE: usize = 0;
const Q15: usize = 1;
const Q14: usize = 2;
const SWITCHES: usize = 3;
const COMBOS: usize = 1 << SWITCHES;

const R122: f32 = 1.0e3;
/// D2's forward drop while C9 charges.
const D2_DROP: f32 = 0.6;
/// A conducting transistor holds its node here: its saturation voltage plus
/// D3's drop at the tens of microamps R28 passes.
const VCA_LOW: f32 = 0.45;
/// R31 (100 Ω) and the diode's resistance at those currents.
const R_ON: f32 = 1.0e3;

// The output: C75 and R116 into the level pot, then C76 into R117 at IC6's
// input. Only C76's high-pass falls in the audio band.
const C76: f32 = 0.022e-6;
const R117: f32 = 22.0e3;

/// Rate the cowbell circuit is simulated at, Hz: fine enough that the
/// oscillators' switching edges land close to their true times.
const CB_RATE: f32 = 96_000.0;

/// The bell is spent once C9 is within this of the clamp: its swing is then
/// some 75 dB down.
const SPENT_ABOVE_CLAMP: f32 = 1.0e-3;
const SETTLED_OUT: f32 = 1.0e-3;

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 0.883;

fn network() -> MnaBuilder<UNKNOWNS> {
    let mut b = MnaBuilder::<UNKNOWNS>::default();
    b.capacitor(ENV, GROUND, 0.47e-6)
        .resistor(ENV, C34_NODE, 33.0e3)
        .capacitor(C34_NODE, GROUND, 1.0e-6)
        .resistor(ENV, VCA_A, 100.0e3)
        .resistor(ENV, VCA_B, 100.0e3)
        .resistor(VCA_A, IN_A, 10.0e3)
        .capacitor(IN_A, M, 0.0022e-6)
        .resistor(VCA_B, IN_B, 10.0e3)
        .capacitor(IN_B, M, 0.0022e-6)
        .resistor(M, GROUND, 2.2e3)
        .capacitor(M, OUT, 0.0033e-6)
        .capacitor(M, INV, 0.01e-6)
        .resistor(INV, OUT, 470.0e3)
        .opamp(GROUND, INV, OUT, OPAMP);
    b
}

/// The cowbell's circuit state.
#[derive(Clone, Copy, Default)]
struct Bell {
    pulse: Pulse,
    net: MnaState<CAPS>,
    env: f32,
    vca_a: f32,
    vca_b: f32,
    out: f32,
    couple: f32,
}

#[derive(Default)]
struct BellShared {
    timing: Timing,
    substeps: u32,
    /// Seconds per substep.
    dt: f32,
    net: Mna<UNKNOWNS, CAPS, SWITCHES, COMBOS>,
    couple_coeff: f32,
}

/// The oscillators at the start of the sample: phase in cycles and advance
/// per sample. They run freely between hits, like the 808's.
struct BellControls {
    low: (f32, f32),
    high: (f32, f32),
}

/// Whether an oscillator is high `frac` of the way through the sample.
#[inline]
fn osc_high((phase, inc): (f32, f32), frac: f32) -> bool {
    (phase + inc * frac).fract() < DUTY
}

/// Brings a fully accented hit to twice the level of an unaccented one.
const ACCENT_TRIM: f32 = 0.875;

impl Fire for Bell {
    fn accent_trim(_: &BellShared) -> f32 {
        ACCENT_TRIM
    }

    fn fire(&mut self, accent: f32, shared: &BellShared) {
        let volts = metal_pulse(pulse_volts(accent));
        self.pulse.fire(volts, shared.timing.trigger_samples);
    }
}

impl Circuit for Bell {
    type Shared = BellShared;
    type Controls = BellControls;

    fn settled(&self) -> bool {
        // C9 drains toward the clamp, where the swing it can give the VCAs,
        // and so the bell, has faded away; it holds what charge it has left.
        !self.pulse.active()
            && self.env < VCA_LOW + SPENT_ABOVE_CLAMP
            && self.out.abs() < SETTLED_OUT
            && self.couple.abs() < SETTLED_OUT
    }

    fn step(&mut self, sh: &BellShared, osc: &BellControls) -> f32 {
        let vtrig = self.pulse.next();
        let charge_to = if vtrig > 0.0 { vtrig - D2_DROP } else { 0.0 };
        let mut sum = 0.0;
        for k in 0..sh.substeps {
            let phase = (k as f32 + 0.5) / sh.substeps as f32;
            let mut closed = 0;
            if charge_to > self.env {
                closed |= 1 << CHARGE;
            }
            // Each transistor conducts while its oscillator is high, and its
            // diode only while the node sits above the clamp.
            if osc_high(osc.low, phase) && self.vca_a > VCA_LOW {
                closed |= 1 << Q15;
            }
            if osc_high(osc.high, phase) && self.vca_b > VCA_LOW {
                closed |= 1 << Q14;
            }
            let mut injected = [0.0; UNKNOWNS];
            if closed & (1 << CHARGE) != 0 {
                injected[ENV] = charge_to / R122;
            }
            let x = sh.net.step(&mut self.net, closed, &injected);
            self.env = x[ENV];
            self.vca_a = x[VCA_A];
            self.vca_b = x[VCA_B];
            self.out = x[OUT];
            sum += high_pass(&mut self.couple, x[OUT], sh.couple_coeff);
        }
        sum / sh.substeps as f32
    }
}
