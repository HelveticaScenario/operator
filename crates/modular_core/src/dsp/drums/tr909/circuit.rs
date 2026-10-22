//! Building blocks shared by the TR-909 analog voice models: the trigger
//! and accent the CPU sends each voice, the shared noise generator, and the
//! transistor VCAs the voices are built around.

use crate::dsp::drums::analog::{Exp, Junction, VT, converged};
use crate::dsp::drums::lanes::{Lanes, Mask, Real, c};
use crate::dsp::drums::voice::{FADE_SECS, quiet_fall, samples};

/// The 909's trigger: a 5 V pulse 2 ms long, which each voice turns into a
/// 15 V pulse on its own trigger transistors.
pub const TRIG_SECS: f32 = 2.0e-3;
pub const TRIG_V: f32 = 15.0;

/// The supply rails.
pub const RAIL: f32 = 15.0;
/// Output swing of the voices' op-amps on the ±15 V rails.
pub const VSAT: f32 = 13.5;
/// Forward drop of a conducting silicon diode.
pub const DIODE_DROP: f32 = 0.6;

/// The accent voltage a voice receives from its accent latch with full
/// accent. Each voice sets its own unaccented voltage so that full accent
/// doubles its level.
pub const ACCENT_HIGH: f32 = 4.0;

/// The accent voltage for a hit with `accent`, 0–1, rising from `low`.
pub fn accent_volts(low: f32, accent: f32) -> f32 {
    low + (ACCENT_HIGH - low) * accent
}

/// Per-sample-rate timing shared by every voice's channels.
#[derive(Clone, Copy, Default)]
pub struct Timing {
    pub trig_samples: u32,
    pub fade_samples: u32,
    /// [`quiet_fall`] at the sample rate.
    pub quiet_fall: f32,
}

impl Timing {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            trig_samples: samples(TRIG_SECS, sample_rate),
            fade_samples: samples(FADE_SECS, sample_rate),
            quiet_fall: quiet_fall(sample_rate),
        }
    }
}

/// A transistor used as the 909's VCA: its emitter at an op-amp's virtual
/// ground, its collector on a node a few millivolts from ground, and its
/// base fed the envelope through a resistor. With both junctions forward
/// biased its collector-emitter path is a conductance set by the base
/// current, from the Ebers-Moll model.
#[derive(Clone, Copy)]
pub struct TransistorVca {
    /// Forward and reverse current gain.
    pub beta_f: f64,
    pub beta_r: f64,
}

/// The 2SC945's saturation current.
const IS: f64 = 10.0e-15;

/// A [`TransistorVca`]'s currents at one operating point.
pub struct VcaCurrents<L: Lanes> {
    /// Into the collector.
    pub collector: L::D,
    /// Out of the emitter, into the virtual ground.
    pub emitter: L::D,
    /// d(collector)/d(collector voltage).
    pub slope: L::D,
}

impl TransistorVca {
    /// The base current drawn through `ohms` from an envelope at `volts`,
    /// with both junctions conducting, by Newton on the base-emitter
    /// junction from its last solution.
    #[inline]
    pub fn base_current<L: Lanes>(&self, volts: L::S, ohms: f32, base: &mut Junction<L>) -> L::D {
        self.base_current_where(!L::DM::default(), volts, ohms, base)
    }

    /// [`Self::base_current`] in the lanes of `on`; the others draw none
    /// and keep their junction.
    #[inline(always)]
    pub fn base_current_where<L: Lanes>(
        &self,
        on: L::DM,
        volts: L::S,
        ohms: f32,
        base: &mut Junction<L>,
    ) -> L::D {
        let (v, r) = (L::wide(volts), ohms as f64);
        let a = r * IS * (1.0 / self.beta_f + 1.0 / self.beta_r);
        let live = on & v.gt(c(0.0));
        // An envelope at or below zero leaves the junction off.
        let off = L::D::select(on, c(0.0), base.v);
        if !live.any() {
            base.v = off;
            return c(0.0);
        }
        // From a cold start, begin near a conducting junction's drop: from
        // zero, Newton would climb the exponential a thermal voltage at a time.
        let mut vbe = L::D::select(base.v.gt(c(0.3)), base.v.clamp(c(0.0), v), v.min(c(0.6)));
        let mut e = base.e.at(vbe, INV_VT);
        let mut done = !live;
        for _ in 0..30 {
            let f = vbe + (e.value - 1.0) * a - v;
            let df = e.value * a * INV_VT + 1.0;
            let next = (vbe - f / df).clamp(c(0.0), v);
            let moved = next - vbe;
            vbe = L::D::select(done, vbe, next);
            done = done | (moved * moved).lt(c(BASE_DONE));
            if done.all() {
                break;
            }
            e = e.at_where(!done, vbe, INV_VT);
        }
        *base = Junction {
            v: L::D::select(live, vbe, off),
            e: Exp::select(live, e, base.e),
        };
        L::D::select(live, (v - vbe) / r, c(0.0))
    }

    /// The currents with base current `ib` and the collector at `vc`, given
    /// as its exponential at scale [`COLLECTOR`].
    #[inline]
    pub fn currents<L: Lanes>(&self, ib: L::D, k: Exp<L>) -> VcaCurrents<L> {
        let k = k.value;
        let (rf, rr) = (1.0 / self.beta_f, 1.0 / self.beta_r);
        let inv_d = c::<L::D>(1.0) / (k * rr + rf);
        let x = (ib + IS * (rf + rr)) * inv_d * (1.0 / IS);
        let gain = c::<L::D>(1.0) - k * (1.0 + rr);
        let collector = (x * gain + rr) * IS;
        let dx = x * k * rr * INV_VT * inv_d;
        let slope = (dx * gain + x * k * (1.0 + rr) * INV_VT) * IS;
        VcaCurrents {
            collector,
            emitter: collector + ib,
            slope,
        }
    }
}

const INV_VT: f64 = 1.0 / VT as f64;

const BASE_DONE: f64 = converged(1.0e-6, VT as f64);
const COLLECTOR_DONE: f64 = converged(1.0e-8, VT as f64);
/// The shaper's diode node couples into its collector node, so it settles
/// a decade tighter than its own tolerance.
const DIODE_DONE: f64 = converged(1.0e-7, DIODE_NVT);

/// The scale of the exponential of a VCA's collector voltage that
/// [`TransistorVca::currents`] takes.
pub const COLLECTOR: f64 = -INV_VT;

/// The 909 voices' transistors.
pub const VCA: TransistorVca = TransistorVca {
    beta_f: 300.0,
    beta_r: 6.0,
};

/// Fraction of a log-taper ("A") pot's resistance at `position`, 0–1: a
/// tenth of it at mid travel.
fn log_taper(position: f32) -> f32 {
    const K: f32 = 4.394;
    ((K * position.clamp(0.0, 1.0)).exp() - 1.0) / (K.exp() - 1.0)
}

/// A log-taper pot of `pot` ohms in series with `fixed` ohms, as a
/// conductance, computed only when its position moves.
#[derive(Clone, Copy)]
pub struct TaperedPot {
    position: f32,
    conductance: f32,
}

impl Default for TaperedPot {
    fn default() -> Self {
        // NaN never compares equal, so the first read always computes.
        Self {
            position: f32::NAN,
            conductance: 0.0,
        }
    }
}

impl TaperedPot {
    #[inline]
    pub fn conductance(&mut self, position: f32, fixed: f32, pot: f32) -> f32 {
        if position != self.position {
            self.position = position;
            self.conductance = 1.0 / (fixed + pot * log_taper(position));
        }
        self.conductance
    }
}

/// Two diodes back to back to ground, rounding a triangle toward a sine:
/// the triangle through `r_in` onto the diodes, then `r_out` into a VCA's
/// collector node with `r_load` to ground.
#[derive(Clone, Copy)]
pub struct Shaper {
    pub r_in: f64,
    pub r_out: f64,
    pub r_load: f64,
}

/// The shaper diodes' saturation current and emission coefficient times
/// the thermal voltage.
const DIODE_IS: f64 = 2.52e-9;
const DIODE_NVT: f64 = 1.752 * VT as f64;

/// A diode pair's current and slope at `e`, the exponential of the voltage
/// across it at scale [`DIODE`].
#[inline]
pub fn diode_pair<L: Lanes>(e: Exp<L>) -> (L::D, L::D) {
    let (e, inv) = (e.value, c::<L::D>(1.0) / e.value);
    (
        (e - inv) * DIODE_IS,
        (e + inv) * (DIODE_IS * (1.0 / DIODE_NVT)),
    )
}

/// The scale of the exponential [`diode_pair`] takes.
pub const DIODE: f64 = 1.0 / DIODE_NVT;

impl Shaper {
    /// Solves the diode node `d` and the collector node `vc` for the
    /// triangle at `vi` and the VCA's base current `ib`, by Newton from
    /// their last values, in the lanes of `on`; the others keep their
    /// nodes. Returns the VCA's emitter current.
    #[inline]
    pub fn solve<L: Lanes>(
        &self,
        on: L::DM,
        d: &mut Junction<L>,
        vc: &mut Junction<L>,
        vi: L::D,
        ib: L::D,
    ) -> L::D {
        self.solve_with(on, d, vc, vi, c(0.0), 0.0, ib)
    }

    /// [`Self::solve`] with a source at `volts` also feeding the collector
    /// node through conductance `g`.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub fn solve_with<L: Lanes>(
        &self,
        on: L::DM,
        d: &mut Junction<L>,
        vc: &mut Junction<L>,
        vi: L::D,
        volts: L::D,
        g: f64,
        ib: L::D,
    ) -> L::D {
        let (g_in, g_out) = (1.0 / self.r_in, 1.0 / self.r_out);
        let g_node = g_out + g + 1.0 / self.r_load;
        let (mut dn, mut cn) = (d.v, vc.v);
        let (mut e, mut k) = (d.e.at(dn, DIODE), vc.e.at(cn, COLLECTOR));
        let mut done = !on;
        for _ in 0..20 {
            let (id, did) = diode_pair(e);
            let q = VCA.currents(ib, k);
            let f1 = (vi - dn) * g_in - id - (dn - cn) * g_out;
            let f2 = dn * g_out + volts * g - cn * g_node - q.collector;
            let a11 = c::<L::D>(-g_in) - did - g_out;
            let a22 = c::<L::D>(-g_node) - q.slope;
            let inv_det = c::<L::D>(1.0) / (a11 * a22 - g_out * g_out);
            let step_d = (f1 * a22 - f2 * g_out) * inv_det;
            let step_c = (a11 * f2 - f1 * g_out) * inv_det;
            dn = L::D::select(done, dn, dn - step_d.clamp(c(-0.1), c(0.1)));
            cn = L::D::select(done, cn, cn - step_c.clamp(c(-0.01), c(0.01)));
            k = k.at_where(!done, cn, COLLECTOR);
            done = done
                | ((step_d * step_d).lt(c(DIODE_DONE)) & (step_c * step_c).lt(c(COLLECTOR_DONE)));
            if done.all() {
                break;
            }
            e = e.at_where(!done, dn, DIODE);
        }
        *d = Junction {
            v: dn,
            e: Exp::select(on, e, d.e),
        };
        *vc = Junction {
            v: cn,
            e: Exp::select(on, k, vc.e),
        };
        VCA.currents(ib, k).emitter
    }
}

/// Solves a VCA's collector node `vc`, fed from the open-circuit voltage
/// `open` through `ohms`, with `r_load` to ground and base current `ib`, in
/// the lanes of `on`; the others keep their node. Returns the emitter
/// current and the current drawn from the source.
#[inline(always)]
pub fn vca_port<L: Lanes>(
    on: L::DM,
    vc: &mut Junction<L>,
    open: L::D,
    ohms: L::D,
    r_load: f64,
    ib: L::D,
) -> (L::D, L::D) {
    let g_source = c::<L::D>(1.0) / ohms;
    let g = g_source + 1.0 / r_load;
    let mut v = vc.v;
    let mut k = vc.e.at(v, COLLECTOR);
    let mut done = !on;
    for _ in 0..20 {
        let q = VCA.currents(ib, k);
        let f = open * g_source - v * g - q.collector;
        let step = (f / (-g - q.slope)).clamp(c(-0.01), c(0.01));
        v = L::D::select(done, v, v - step);
        k = k.at_where(!done, v, COLLECTOR);
        done = done | (step * step).lt(c(COLLECTOR_DONE));
        if done.all() {
            break;
        }
    }
    *vc = Junction {
        v,
        e: Exp::select(on, k, vc.e),
    };
    (VCA.currents(ib, k).emitter, (open - v) * g_source)
}
