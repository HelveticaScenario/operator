//! Building blocks shared by the TR-909 analog voice models: the trigger
//! and accent the CPU sends each voice, the shared noise generator, and the
//! transistor VCAs the voices are built around.

use crate::dsp::drums::analog::VT;
use crate::dsp::drums::voice::{CHOKE_SECS, quiet_fall, samples};
use crate::dsp::utils::rng::LcgRng;

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
    pub choke_samples: u32,
    /// [`quiet_fall`] at the sample rate.
    pub quiet_fall: f32,
}

impl Timing {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            trig_samples: samples(TRIG_SECS, sample_rate),
            choke_samples: samples(CHOKE_SECS, sample_rate),
            quiet_fall: quiet_fall(sample_rate),
        }
    }
}

/// The 909's noise generator: two 4006 shift registers chained into 32
/// stages with exclusive-or feedback, clocked by an RC oscillator on IC31
/// (R185 33 kΩ, C47 100 pF), its output swinging the full 15 V.
#[derive(Clone, Copy)]
pub struct Noise {
    register: u32,
    /// Clock phase, in clock periods.
    phase: f32,
    seeded: bool,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            register: TOP,
            phase: 0.0,
            seeded: false,
        }
    }
}

/// A 31-stage maximal-length register stands in for the 909's 32 stages:
/// its sequence repeats only after 2^31 clocks, some hours.
const STAGES_MASK: u32 = (1 << 31) - 1;
const TOP: u32 = 1 << 30;

/// The shift registers' clock, Hz.
const NOISE_CLOCK_HZ: f32 = 1.0 / (2.2 * 33.0e3 * 100.0e-12);

impl Noise {
    /// Seeds the register unless it already is. The register rides patch
    /// updates with its channel, so a carried-over channel keeps its
    /// sequence and a channel added by an update gets its own.
    pub fn seed(&mut self, base: u64, stream: usize) {
        if self.seeded {
            return;
        }
        self.seeded = true;
        let mut rng = LcgRng::default();
        rng.seed(base, stream);
        // Any state but all zeros runs; the top bit keeps it off zero.
        self.register = (((rng.next_bipolar() * 1.0e9) as i64 as u32) & STAGES_MASK) | TOP;
    }

    /// Clock periods per sample at `sample_rate`.
    pub fn rate(sample_rate: f32) -> f32 {
        NOISE_CLOCK_HZ / sample_rate
    }

    /// The register's output averaged over one sample, in volts: the
    /// filters that follow it are all far below the clock.
    #[inline]
    pub fn next(&mut self, rate: f32) -> f32 {
        let end = self.phase + rate;
        let mut high = 0.0;
        let mut t = self.phase;
        while t < end {
            let edge = t.floor() + 1.0;
            let span = edge.min(end) - t;
            if self.register & 1 != 0 {
                high += span;
            }
            if edge <= end {
                // Feedback from stages 31 and 28.
                let bit = (self.register ^ (self.register >> 3)) & 1;
                self.register = (self.register >> 1) | (bit << 30);
            }
            t = edge;
        }
        self.phase = end.fract();
        RAIL * high / rate
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
pub struct VcaCurrents {
    /// Into the collector.
    pub collector: f64,
    /// Out of the emitter, into the virtual ground.
    pub emitter: f64,
    /// d(collector)/d(collector voltage).
    pub slope: f64,
}

impl TransistorVca {
    /// The base current drawn through `ohms` from an envelope at `volts`,
    /// with both junctions conducting, by Newton on the base voltage from
    /// `guess`.
    pub fn base_current(&self, volts: f32, ohms: f32, guess: &mut f64) -> f64 {
        let (v, r, vt) = (volts as f64, ohms as f64, VT as f64);
        let is_b = IS * (1.0 / self.beta_f + 1.0 / self.beta_r);
        if v <= 0.0 {
            *guess = 0.0;
            return 0.0;
        }
        // From a cold start, begin near a conducting junction's drop: from
        // zero, Newton would climb the exponential a thermal voltage at a time.
        let mut vbe = if *guess > 0.3 {
            guess.clamp(0.0, v)
        } else {
            v.min(0.6)
        };
        for _ in 0..30 {
            let e = (vbe / vt).min(80.0).exp();
            let f = vbe + r * is_b * (e - 1.0) - v;
            let df = 1.0 + r * is_b * e / vt;
            let next = (vbe - f / df).clamp(0.0, v);
            let moved = (next - vbe).abs();
            vbe = next;
            if moved < 1.0e-6 {
                break;
            }
        }
        *guess = vbe;
        (v - vbe) / r
    }

    /// The currents with base current `ib` and the collector at `vc`.
    #[inline]
    pub fn currents(&self, ib: f64, vc: f64) -> VcaCurrents {
        let vt = VT as f64;
        let (bf, br) = (self.beta_f, self.beta_r);
        let k = (-vc / vt).clamp(-80.0, 80.0).exp();
        let d = 1.0 / bf + k / br;
        let x = (ib + IS / bf + IS / br) / (IS * d);
        let collector = IS * x * (1.0 - k - k / br) + IS / br;
        let dx = x * k / (vt * br * d);
        let slope = IS * dx * (1.0 - k - k / br) + IS * x * (k / vt + k / (br * vt));
        VcaCurrents {
            collector,
            emitter: collector + ib,
            slope,
        }
    }
}

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

impl Shaper {
    /// Solves the diode node `d` and the collector node `vc` for the
    /// triangle at `vi` and the VCA's base current `ib`, by Newton from
    /// their last values. Returns the VCA's emitter current.
    #[inline]
    pub fn solve(&self, d: &mut f64, vc: &mut f64, vi: f64, ib: f64) -> f64 {
        let (mut dn, mut cn) = (*d, *vc);
        for _ in 0..20 {
            let e = (dn / DIODE_NVT).clamp(-80.0, 80.0).exp();
            let id = DIODE_IS * (e - 1.0 / e);
            let did = DIODE_IS * (e + 1.0 / e) / DIODE_NVT;
            let q = VCA.currents(ib, cn);
            let f1 = (vi - dn) / self.r_in - id - (dn - cn) / self.r_out;
            let f2 = (dn - cn) / self.r_out - cn / self.r_load - q.collector;
            let (a11, a12) = (-1.0 / self.r_in - did - 1.0 / self.r_out, 1.0 / self.r_out);
            let (a21, a22) = (
                1.0 / self.r_out,
                -1.0 / self.r_out - 1.0 / self.r_load - q.slope,
            );
            let det = a11 * a22 - a12 * a21;
            let step_d = (f1 * a22 - f2 * a12) / det;
            let step_c = (a11 * f2 - a21 * f1) / det;
            dn -= step_d.clamp(-0.1, 0.1);
            cn -= step_c.clamp(-0.01, 0.01);
            if step_d.abs() < 1.0e-6 && step_c.abs() < 1.0e-8 {
                break;
            }
        }
        *d = dn;
        *vc = cn;
        VCA.currents(ib, cn).emitter
    }
}

/// Solves a VCA's collector node `vc`, fed from the open-circuit voltage
/// `open` through `ohms`, with `r_load` to ground and base current `ib`.
/// Returns the emitter current and the current drawn from the source.
#[inline]
pub fn vca_port(vc: &mut f64, open: f64, ohms: f64, r_load: f64, ib: f64) -> (f64, f64) {
    let mut v = *vc;
    for _ in 0..20 {
        let q = VCA.currents(ib, v);
        let f = (open - v) / ohms - v / r_load - q.collector;
        let df = -1.0 / ohms - 1.0 / r_load - q.slope;
        let step = (f / df).clamp(-0.01, 0.01);
        v -= step;
        if step.abs() < 1.0e-8 {
            break;
        }
    }
    *vc = v;
    (VCA.currents(ib, v).emitter, (open - v) / ohms)
}
