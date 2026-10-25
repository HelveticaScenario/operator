//! The "swing-type" VCAs (the service notes' name) of the 808's cymbal and
//! hi-hats: a 2SC945 whose base takes a band of the oscillator mix through
//! 0.022 µF, biased from the 15 V rail through 2 MΩ, with 100 Ω in its
//! emitter, and whose collector pulls a node fed from an envelope down
//! through a 1S2473 diode.
//!
//! On each positive swing of the band the base-emitter junction clamps the
//! base, charging the coupling capacitor; the collector, which the envelope
//! can only feed tens of microamps, saturates, so the base carries whatever
//! emitter current the collector cannot take. The collector current is
//! what pulls the envelope's node down, so the VCA's output swings between
//! the envelope and a saturation floor in time with the band's peaks.

use crate::dsp::drums::analog::{Exp, Junction, VT, converged};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{Mna, MnaState};

/// The 2SC945's saturation current and forward current gain.
const IS: f64 = 10.0e-15;
const ALPHA: f64 = 300.0 / 301.0;
const RAIL: f64 = 15.0;
const C_COUPLE: f64 = 0.022e-6;
const R_BIAS: f64 = 2.0e6;
const R_EMIT: f64 = 100.0;
/// Newton's tolerance and iteration limit on the base-emitter voltage.
const DONE: f64 = converged(1.0e-7, VT as f64);
const ITERATIONS: usize = 30;
/// Above this the base-emitter voltage moves at most `LIMIT_STEP` per
/// Newton step, so the junction's exponential cannot overshoot.
const LIMIT_ABOVE: f64 = 0.5;
const LIMIT_STEP: f64 = 2.0 * VT as f64;

/// A saturated VCA transistor's collector voltage; the diode then joins the
/// node to it.
const VCE_SAT: f32 = 0.05;
const DIODE_IS: f32 = 2.52e-9;
const DIODE_NVT: f32 = 1.752 * VT;
/// No more than this stands across the diode at the currents here.
const DIODE_MAX: f32 = 0.7;

/// A swing VCA's base: the coupling capacitor's trapezoidal history and the
/// base-emitter voltage, with its exponential, on the last sample; and the
/// node voltage that last saturated the collector.
#[derive(Clone, Copy, Default)]
pub struct SwingState<L: Lanes = Mono> {
    couple_v: L::S,
    couple_i: L::S,
    vbe: L::D,
    e: Exp<L>,
    saturated: Junction<L>,
}

lanewise!(SwingState {
    couple_v,
    couple_i,
    vbe,
    e,
    saturated
});

impl SwingState {
    /// The base at rest with no band and no envelope: R_BIAS's current all
    /// flows out through the emitter.
    pub fn rest() -> Self {
        let vt = VT as f64;
        let mut vbe = 0.5f64;
        for _ in 0..100 {
            let e = (vbe / vt).exp();
            let ie = IS * (e - 1.0);
            let f = (RAIL - vbe - R_EMIT * ie) / R_BIAS - ie;
            let df = -(1.0 + R_EMIT * IS * e / vt) / R_BIAS - IS * e / vt;
            vbe -= f / df;
        }
        let base = vbe + R_EMIT * IS * ((vbe / vt).exp() - 1.0);
        Self {
            couple_v: -base as f32,
            couple_i: 0.0,
            vbe,
            e: Exp::default(),
            saturated: Junction::default(),
        }
    }
}

impl<L: Lanes> SwingState<L> {
    /// Advances one time step `dt` with the band at `band` and the collector
    /// able to sink at most `available` amps, returning the collector
    /// current.
    #[inline(always)]
    fn step(&mut self, band: L::S, available: L::S, dt: f32) -> L::S {
        let gc = 2.0 * C_COUPLE / dt as f64;
        let history = L::wide(self.couple_v) * gc + L::wide(self.couple_i);
        let (band, avail) = (L::wide(band), L::wide(available.max(c(0.0))));
        let inv_vt = 1.0 / VT as f64;
        let g = gc + 1.0 / R_BIAS;
        let mut vbe = self.vbe;
        let mut e = self.e.at(vbe, inv_vt);
        let mut base = c::<L::D>(0.0);
        let mut ic = c::<L::D>(0.0);
        let mut done = L::DM::default();
        for _ in 0..ITERATIONS {
            let ie = (e.value - 1.0) * IS;
            let die = e.value * IS * inv_vt;
            let at = vbe + ie * R_EMIT;
            // The collector takes α·Ie while it can; past that it saturates
            // and the base carries the rest.
            let linear = (ie * ALPHA).lt(avail);
            let ic_now = L::D::select(linear, ie * ALPHA, avail);
            let ib = L::D::select(linear, ie * (1.0 - ALPHA), ie - avail);
            let dib = L::D::select(linear, die * (1.0 - ALPHA), die);
            let f = band * gc - history + RAIL * (1.0 / R_BIAS) - at * g - ib;
            let df = (die * R_EMIT + 1.0) * -g - dib;
            let next = vbe - f / df;
            let limited = vbe + (next - vbe).clamp(c(-LIMIT_STEP * 8.0), c(LIMIT_STEP));
            let next = L::D::select(next.gt(c(LIMIT_ABOVE)), limited, next);
            let moved = next - vbe;
            vbe = L::D::select(done, vbe, next);
            base = L::D::select(done, base, at);
            ic = L::D::select(done, ic, ic_now);
            done = done | (moved * moved).lt(c(DONE));
            if done.all() {
                break;
            }
            e = e.at_where(!done, vbe, inv_vt);
        }
        self.vbe = vbe;
        self.e = e;
        let v = L::narrow(band - base);
        self.couple_i = L::narrow(L::wide(v) * gc - history);
        self.couple_v = v;
        L::narrow(ic)
    }
}

/// The most current a VCA's node can push through its diode into a
/// saturated collector, from the node's open-circuit voltage `open` and
/// impedance `z`, by Newton on the node voltage from its last solution.
#[inline(always)]
fn saturated_current<L: Lanes>(open: L::S, z: L::S, node: &mut Junction<L>) -> L::S {
    let on = open.gt(c(VCE_SAT));
    if !on.any() {
        return c(0.0);
    }
    // Newton on the node voltage n: n = open − z·Id(n − VCE_SAT).
    const DONE: f64 = converged(1.0e-6, DIODE_NVT as f64);
    let (open, z, sat) = (L::wide(open), L::wide(z), VCE_SAT as f64);
    let a = z * DIODE_IS as f64;
    let inv_nvt = 1.0 / DIODE_NVT as f64;
    let mut n = node.v.clamp(c(sat), open.min(c(sat + DIODE_MAX as f64)));
    let mut e = node.e.at(n - sat, inv_nvt);
    let on_d = L::wide_mask(on);
    let mut done = !on_d;
    for _ in 0..12 {
        let f = n - open + (e.value - 1.0) * a;
        let df = e.value * a * inv_nvt + 1.0;
        let next = (n - f / df).clamp(c(sat), open);
        let moved = next - n;
        n = L::D::select(done, n, next);
        done = done | (moved * moved).lt(c(DONE));
        if done.all() {
            break;
        }
        e = e.at_where(!done, n - sat, inv_nvt);
    }
    *node = Junction {
        v: L::D::select(on_d, n, node.v),
        e: Exp::select(on_d, e, node.e),
    };
    L::S::select(on, L::narrow((open - n) / z), c(0.0))
}

/// Solves one sample of a VCA whose collector pulls on unknown `node` of
/// `net`: the node's open-circuit voltage and impedance give how much
/// current the saturated collector can sink, the base then sets the
/// collector current, and that current pulls the node down.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn vca<L: Lanes, const U: usize, const C: usize, const S: usize, const K: usize>(
    net: &Mna<U, C, S, K>,
    state: &mut MnaState<C, L>,
    closed: L::Closed,
    node: usize,
    base: &mut SwingState<L>,
    band: L::S,
    injected: &[L::S; U],
    dt: f32,
) -> [L::S; U] {
    let x = net.open(state, closed, injected);
    let z = net.impedance::<L>(closed, node, node);
    let available = saturated_current(x[node], z, &mut base.saturated);
    let ic = base.step(band, available, dt);
    net.close(state, closed, x, &[(node, -ic)])
}
