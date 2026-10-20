//! Semiconductor models shared by the drive stage, the filter core and the
//! output amp, taken from the manufacturers' SPICE models.

use std::sync::LazyLock;

use super::lanes::Lanes;

/// Thermal voltage at 27 °C, the temperature the SPICE models are fitted at.
pub(super) const VT: f64 = 0.025_865;

/// 1N4148 small-signal diode (onsemi SPICE model).
pub(super) const DIODE_IS: f64 = 2.52e-9;
pub(super) const DIODE_NVT: f64 = 1.752 * VT;

/// A junction's current law with PSpice's high-injection knee,
/// `i = inrm · √(ikf / (ikf + inrm))` where `inrm = is · (exp(v / nvt) − 1)`.
struct Junction {
    is: f64,
    nvt: f64,
    ikf: f64,
}

impl Junction {
    /// Current and its derivative at junction voltage `v`.
    fn current(&self, v: f64) -> (f64, f64) {
        let e = (v / self.nvt).exp();
        let inrm = self.is * (v / self.nvt).exp_m1();
        let knee = (self.ikf / (self.ikf + inrm)).sqrt();
        let dknee = knee * (1.0 - 0.5 * inrm / (self.ikf + inrm));
        (inrm * knee, dknee * self.is * e / self.nvt)
    }

    /// Junction voltage carrying forward current `i`, and its derivative.
    fn voltage(&self, i: f64) -> (f64, f64) {
        let (i2, ikf) = (i * i, self.ikf);
        let inrm = (i2 + (i2 * i2 + 4.0 * ikf * ikf * i2).sqrt()) / (2.0 * ikf);
        let knee = (ikf / (ikf + inrm)).sqrt();
        let dknee = knee * (1.0 - 0.5 * inrm / (ikf + inrm));
        let v = self.nvt * (inrm / self.is).ln_1p();
        (v, self.nvt / (self.is + inrm) / dknee)
    }
}

/// BZT585B5V1T zener, from Diodes Inc's SPICE model: a forward junction (DF)
/// carrying the depletion capacitance, a soft reverse leakage junction (DR)
/// whose knee makes the 5.1 V part start conducting around 3 V, and the
/// breakdown itself (DR2, with PSpice's low-level breakdown term).
const ZENER_DF: Junction = Junction {
    is: 1.5e-14,
    nvt: 1.1 * VT,
    ikf: 0.12,
};
const ZENER_DF_RS: f64 = 0.32;
const ZENER_DR: Junction = Junction {
    is: 0.46e-12,
    nvt: 7.3 * VT,
    ikf: 4.0e-6,
};
const ZENER_DR_RS: f64 = 1.75;
const ZENER_BV: f64 = 5.1;
const ZENER_IBV: f64 = 100.0e-9;
const ZENER_NBV: f64 = 1.2;
const ZENER_IBVL: f64 = 50.0e-3;
const ZENER_BREAKDOWN_RS: f64 = 2.05;
const ZENER_CJO: f64 = 200.0e-12;
const ZENER_VJ: f64 = 0.4;
const ZENER_M: f64 = 0.5;

/// Breakdown current and its derivative at reverse junction voltage `v`.
fn breakdown(v: f64) -> (f64, f64) {
    let high = ZENER_IBV * ((v - ZENER_BV) / (ZENER_NBV * VT)).exp();
    let low = ZENER_IBVL * ((v - ZENER_BV) / VT).exp();
    (high + low, high / (ZENER_NBV * VT) + low / VT)
}

/// Current through `law` behind series resistance `rs` at terminal voltage
/// `v ≥ 0`, with its derivative.
fn series_current(law: impl Fn(f64) -> (f64, f64), rs: f64, v: f64) -> (f64, f64) {
    let (mut lo, mut hi, mut vj) = (0.0, v, v);
    for _ in 0..200 {
        let (i, di) = law(vj);
        let r = vj + rs * i - v;
        if r.abs() < 1.0e-14 {
            break;
        }
        if r > 0.0 {
            hi = vj;
        } else {
            lo = vj;
        }
        let next = vj - r / (1.0 + rs * di);
        vj = if next > lo && next < hi {
            next
        } else {
            0.5 * (lo + hi)
        };
    }
    let (i, di) = law(vj);
    (i, di / (1.0 + rs * di))
}

/// Reverse current of one zener at terminal voltage `v ≥ 0`, with its
/// derivative.
fn zener_reverse_current(v: f64) -> (f64, f64) {
    let (leak, dleak) = series_current(|vj| ZENER_DR.current(vj), ZENER_DR_RS, v);
    let (brk, dbrk) = series_current(breakdown, ZENER_BREAKDOWN_RS, v);
    let (df, ddf) = ZENER_DF.current(-v);
    (leak + brk - df, dleak + dbrk + ddf)
}

/// Reverse terminal voltage of one zener carrying `i`, and `dv/di`, starting
/// the search from `seed`.
fn zener_reverse_voltage(i: f64, seed: f64) -> (f64, f64) {
    let (mut lo, mut hi, mut v) = (0.0, 10.0, seed.clamp(0.0, 10.0));
    for _ in 0..200 {
        let (iv, div) = zener_reverse_current(v);
        let r = iv - i;
        if r.abs() <= 1.0e-12 * i {
            break;
        }
        if r > 0.0 {
            hi = v;
        } else {
            lo = v;
        }
        let next = v - r / div;
        v = if next > lo && next < hi {
            next
        } else {
            0.5 * (lo + hi)
        };
    }
    (v, 1.0 / zener_reverse_current(v).1)
}

/// Depletion capacitance of one zener's junction at junction voltage `v`, with
/// SPICE's linear extension above half the built-in potential.
fn depletion_capacitance(v: f64) -> f64 {
    const FC: f64 = 0.5;
    if v < FC * ZENER_VJ {
        ZENER_CJO * (1.0 - v / ZENER_VJ).powf(-ZENER_M)
    } else {
        ZENER_CJO / (1.0 - FC).powf(1.0 + ZENER_M)
            * (1.0 - FC * (1.0 + ZENER_M) + ZENER_M * v / ZENER_VJ)
    }
}

/// A back-to-back zener pair (one forward, one reverse) at one operating
/// point.
pub(super) struct ZenerPoint {
    pub current: f64,
    pub voltage: f64,
    /// d(voltage) / d(ln current).
    pub slope: f64,
}

/// Lowest current [`ZenerTables::pair`] models (10 nA, about 2.27 V across the
/// pair).
/// Below it the pair carries too little to matter against the tens of kΩ
/// around it.
pub(super) const ZENER_MIN_LN_I: f64 = -18.420_680_743_952_367;

/// Voltage across the pair at [`ZENER_MIN_LN_I`], below which it is treated as
/// open.
pub(super) const ZENER_FLOOR_V: f64 = 2.267_45;

/// Log-current grid spacing of the pair tables.
const PER_LN_I: f64 = 64.0;
/// The capacitance sweep starts this many grid steps below the 10 nA floor,
/// at about 0 V.
const SWEEP_BELOW_FLOOR: usize = 2560;
/// Grid steps from the floor up to 1 A.
const PAIR_STEPS: usize = 1179;
/// Capacitance table spacing and extent, in pair volts.
const CAP_PER_VOLT: f64 = 128.0;
const CAP_MAX_V: f64 = 6.0;

pub(super) struct ZenerTables {
    /// Pair `(voltage, d voltage / d ln current)` on the log-current grid from
    /// [`ZENER_MIN_LN_I`].
    pair: Box<[(f64, f64)]>,
    /// Pair capacitance on a uniform voltage grid from 0 V.
    capacitance: Box<[f32]>,
}

static ZENER: LazyLock<ZenerTables> = LazyLock::new(|| {
    let steps = SWEEP_BELOW_FLOOR + PAIR_STEPS + 1;
    let mut voltage = Vec::with_capacity(steps);
    let mut pair = Vec::with_capacity(PAIR_STEPS + 1);
    let mut cap = Vec::with_capacity(steps);
    let mut seed = 0.0;
    for k in 0..steps {
        let ln_i = ZENER_MIN_LN_I + (k as f64 - SWEEP_BELOW_FLOOR as f64) / PER_LN_I;
        let i = ln_i.exp();
        let (vf, dvf) = ZENER_DF.voltage(i);
        let (vr, dvr) = zener_reverse_voltage(i, seed);
        seed = vr;
        let v = vf + ZENER_DF_RS * i + vr;
        let (ca, cb) = (depletion_capacitance(vf), depletion_capacitance(-vr));
        voltage.push(v);
        cap.push(ca * cb / (ca + cb));
        if k >= SWEEP_BELOW_FLOOR {
            pair.push((v, i * (dvf + ZENER_DF_RS + dvr)));
        }
    }
    let mut k = 0;
    let capacitance = (0..=(CAP_MAX_V * CAP_PER_VOLT) as usize)
        .map(|n| {
            let v = n as f64 / CAP_PER_VOLT;
            while k + 2 < steps && voltage[k + 1] <= v {
                k += 1;
            }
            let t = ((v - voltage[k]) / (voltage[k + 1] - voltage[k])).clamp(0.0, 1.0);
            (cap[k] + (cap[k + 1] - cap[k]) * t) as f32
        })
        .collect();
    ZenerTables {
        pair: pair.into_boxed_slice(),
        capacitance,
    }
});

/// Build the zener tables now (main thread) so the audio thread never
/// triggers it.
pub(super) fn prime() {
    LazyLock::force(&ZENER);
}

/// The zener tables, built by [`prime`].
pub(super) fn zener_tables() -> &'static ZenerTables {
    &ZENER
}

impl ZenerTables {
    /// The pair's operating point at current `exp(ln_i)`, for `ln_i` at or
    /// above [`ZENER_MIN_LN_I`]: cubic Hermite interpolation of the tabulated
    /// curve, extended linearly in `ln_i` above 1 A.
    #[inline]
    pub(super) fn pair(&self, ln_i: f64) -> ZenerPoint {
        let table = &self.pair;
        let pos = (ln_i - ZENER_MIN_LN_I) * PER_LN_I;
        let k = (pos as usize).min(table.len() - 2);
        let t = pos - k as f64;
        let ((v0, s0), (v1, s1)) = (table[k], table[k + 1]);
        let h = 1.0 / PER_LN_I;
        let (voltage, slope) = if t > 1.0 {
            (v1 + s1 * (t - 1.0) * h, s1)
        } else {
            let (t2, t3) = (t * t, t * t * t);
            let voltage = (2.0 * t3 - 3.0 * t2 + 1.0) * v0
                + (t3 - 2.0 * t2 + t) * h * s0
                + (3.0 * t2 - 2.0 * t3) * v1
                + (t3 - t2) * h * s1;
            let slope = ((6.0 * t2 - 6.0 * t) * (v0 - v1)) / h
                + (3.0 * t2 - 4.0 * t + 1.0) * s0
                + (3.0 * t2 - 2.0 * t) * s1;
            (voltage, slope)
        };
        ZenerPoint {
            current: ln_i.exp(),
            voltage,
            slope,
        }
    }

    /// Capacitance of the pair with `v` across it, per lane. The
    /// reverse-biased junction's depletion layer widens with voltage, so it
    /// falls from 100 pF at 0 V to about 55 pF at 4.5 V; past the table it
    /// holds its last value.
    #[inline]
    pub(super) fn capacitance<L: Lanes>(&self, v: L) -> L {
        L::lerp(&self.capacitance, v.abs() * L::splat(CAP_PER_VOLT as f32))
    }
}

/// Voltage across the pair carrying current `exp(ln_i)`.
pub(super) fn zener_pair_voltage(ln_i: f64) -> f64 {
    ZENER.pair(ln_i).voltage
}

/// Output magnitude of a transimpedance amp whose feedback is `r` across
/// `pairs` zener pairs in series, for unclamped target `x ≥ 0` (`r` times the
/// input current): solves `v + r · i(v / pairs) = x`, parametrised by the pair
/// current so both sides are explicit and monotonic.
pub(super) fn zener_clamp(x: f64, pairs: f64, r: f64) -> f64 {
    let (mut lo, mut hi) = (ZENER_MIN_LN_I, 0.0_f64);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if pairs * zener_pair_voltage(mid) + r * mid.exp() < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (pairs * zener_pair_voltage(0.5 * (lo + hi))).min(x)
}

/// Small-signal capacitance of the pair at 0 V: two zero-bias junctions in
/// series.
pub(super) const ZENER_PAIR_CAPACITANCE_0V: f32 = (ZENER_CJO / 2.0) as f32;

#[cfg(test)]
mod tests {
    use super::*;

    /// Pair voltages from ngspice running the vendor model, at 10 nA to 20 mA.
    #[test]
    fn pair_matches_the_vendor_model() {
        let reference = [
            (1.0e-8, 2.267408),
            (1.0e-6, 3.291291),
            (1.0e-5, 3.965844),
            (1.0e-4, 4.876586),
            (1.0e-3, 5.677860),
            (5.0e-3, 5.801174),
            (2.0e-2, 5.916929),
        ];
        for (i, v) in reference {
            let got = zener_pair_voltage(f64::ln(i));
            assert!((got - v).abs() < 1.0e-4, "{i} A: {got} V vs {v} V");
        }
    }

    #[test]
    fn floor_voltage_is_the_pair_at_its_floor() {
        assert!((ZENER.pair[0].0 - ZENER_FLOOR_V).abs() < 1.0e-5);
    }

    #[test]
    fn single_zener_meets_the_datasheet_limits() {
        // 5.00–5.20 V at 5 mA, under 2 µA of leakage at 2 V.
        let (v5, _) = zener_reverse_voltage(5.0e-3, 5.0);
        assert!((5.0..5.2).contains(&v5), "{v5} V at 5 mA");
        assert!(zener_reverse_current(2.0).0 < 2.0e-6);
    }

    #[test]
    fn slope_matches_finite_difference() {
        for ln_i in [-18.0, -14.3, -9.0, -6.0, -3.0] {
            let h = 1.0e-5;
            let fd = (zener_pair_voltage(ln_i + h) - zener_pair_voltage(ln_i - h)) / (2.0 * h);
            let slope = ZENER.pair(ln_i).slope;
            assert!(
                (fd - slope).abs() < 1.0e-4 * slope,
                "ln_i={ln_i}: {fd} vs {slope}"
            );
        }
    }

    #[test]
    fn capacitance_falls_with_voltage() {
        let c = |v: f32| ZENER.capacitance(v);
        assert!((c(0.0) - ZENER_PAIR_CAPACITANCE_0V).abs() < 0.5e-12);
        let mut last = f32::INFINITY;
        for n in 0..12 {
            let at = c(n as f32 * 0.5);
            assert!(at <= last, "{at} at {} V", n as f32 * 0.5);
            last = at;
        }
        assert!((40.0e-12..60.0e-12).contains(&c(5.0)));
        // Past the table the last value holds.
        assert_eq!(c(1.0e6), c(6.0));
    }
}
