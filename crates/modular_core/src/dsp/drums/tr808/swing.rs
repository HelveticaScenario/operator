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

use crate::dsp::drums::analog::VT;
use crate::dsp::drums::mna::{Mna, MnaState};

/// The 2SC945's saturation current and forward current gain.
const IS: f64 = 10.0e-15;
const ALPHA: f64 = 300.0 / 301.0;
const RAIL: f64 = 15.0;
const C_COUPLE: f64 = 0.022e-6;
const R_BIAS: f64 = 2.0e6;
const R_EMIT: f64 = 100.0;
/// Newton's tolerance and iteration limit on the base-emitter voltage.
const TOLERANCE: f64 = 1.0e-7;
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
/// base-emitter voltage on the last sample.
#[derive(Clone, Copy, Default)]
pub struct SwingState {
    couple_v: f32,
    couple_i: f32,
    vbe: f64,
}

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
        }
    }

    /// Advances one time step `dt` with the band at `band` and the collector
    /// able to sink at most `available` amps, returning the collector
    /// current.
    #[inline]
    fn step(&mut self, band: f32, available: f32, dt: f32) -> f32 {
        let gc = 2.0 * C_COUPLE / dt as f64;
        let history = gc * self.couple_v as f64 + self.couple_i as f64;
        let (band, avail) = (band as f64, available.max(0.0) as f64);
        let vt = VT as f64;
        let mut vbe = self.vbe;
        let mut base = 0.0;
        let mut ic = 0.0;
        for _ in 0..ITERATIONS {
            let e = (vbe / vt).min(80.0).exp();
            let ie = IS * (e - 1.0);
            let die = IS * e / vt;
            base = vbe + R_EMIT * ie;
            // The collector takes α·Ie while it can; past that it saturates
            // and the base carries the rest.
            let (ib, dib) = if ALPHA * ie < avail {
                ic = ALPHA * ie;
                ((1.0 - ALPHA) * ie, (1.0 - ALPHA) * die)
            } else {
                ic = avail;
                (ie - avail, die)
            };
            let f = gc * (band - base) - history + (RAIL - base) / R_BIAS - ib;
            let df = -(gc + 1.0 / R_BIAS) * (1.0 + R_EMIT * die) - dib;
            let mut next = vbe - f / df;
            if next > LIMIT_ABOVE {
                next = vbe + (next - vbe).clamp(-LIMIT_STEP * 8.0, LIMIT_STEP);
            }
            let moved = (next - vbe).abs();
            vbe = next;
            if moved < TOLERANCE {
                break;
            }
        }
        self.vbe = vbe;
        let v = (band - base) as f32;
        self.couple_i = (gc * v as f64 - history) as f32;
        self.couple_v = v;
        ic as f32
    }
}

/// The most current a VCA's node can push through its diode into a
/// saturated collector, from the node's open-circuit voltage `open` and
/// impedance `z`, starting Newton from `guess` (the node's last voltage).
#[inline]
fn saturated_current(open: f32, z: f32, guess: f32) -> f32 {
    if open <= VCE_SAT {
        return 0.0;
    }
    // Newton on the node voltage n: n = open − z·Id(n − VCE_SAT).
    let mut n = guess.clamp(VCE_SAT, open.min(VCE_SAT + DIODE_MAX));
    for _ in 0..12 {
        let e = ((n - VCE_SAT) / DIODE_NVT).min(80.0).exp();
        let f = n - open + z * DIODE_IS * (e - 1.0);
        let df = 1.0 + z * DIODE_IS * e / DIODE_NVT;
        let next = (n - f / df).clamp(VCE_SAT, open);
        let moved = (next - n).abs();
        n = next;
        if moved < 1.0e-6 {
            break;
        }
    }
    (open - n) / z
}

/// Solves one sample of a VCA whose collector pulls on unknown `node` of
/// `net`: the node's open-circuit voltage and impedance give how much
/// current the saturated collector can sink, the base then sets the
/// collector current, and that current pulls the node down. `last` is the
/// node's voltage on the last sample.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn vca<const U: usize, const C: usize, const S: usize, const K: usize>(
    net: &Mna<U, C, S, K>,
    state: &mut MnaState<C>,
    closed: usize,
    node: usize,
    base: &mut SwingState,
    band: f32,
    injected: &[f32; U],
    last: f32,
    dt: f32,
) -> [f32; U] {
    let x = net.open(state, closed, injected);
    let z = net.impedance(closed, node, node);
    let available = saturated_current(x[node], z, last);
    let ic = base.step(band, available, dt);
    net.close(state, closed, x, &[(node, -ic)])
}
