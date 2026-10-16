//! Semiconductor models shared by the drive stage and the filter core.

/// Thermal voltage at 27 °C.
pub(super) const VT: f64 = 0.025_85;

/// BZT585B5V1T zener (5.1 V at 5 mA): exponential breakdown knee plus bulk
/// resistance.
const ZENER_BV: f64 = 5.1;
const ZENER_IBV: f64 = 5.0e-3;
const ZENER_IS: f64 = 1.0e-14;
const ZENER_RS: f64 = 20.0;

/// Zero-bias junction capacitance of one zener (datasheet CT at 0 V). Its
/// voltage dependence is unspecified, so it is held constant.
pub(super) const ZENER_CAPACITANCE: f32 = 300.0e-12;

/// 1N4148 small-signal diode.
pub(super) const DIODE_IS: f64 = 2.52e-9;
pub(super) const DIODE_NVT: f64 = 1.752 * VT;

/// A back-to-back zener pair (one forward, one in breakdown) at one operating
/// point.
pub(super) struct ZenerPoint {
    pub current: f64,
    pub voltage: f64,
    /// d(voltage) / d(ln current).
    pub slope: f64,
}

/// Lowest current [`zener_pair`] models (1 pA, about 4.7 V across the pair).
pub(super) const ZENER_MIN_LN_I: f64 = -27.631_021_115_928_547;

/// The pair's operating point at current `exp(ln_i)`, valid for `ln_i` at or
/// above [`ZENER_MIN_LN_I`]. Parametrising by log-current keeps the curve
/// explicit and monotonic up to tens of milliamps.
#[inline]
pub(super) fn zener_pair(ln_i: f64) -> ZenerPoint {
    let current = ln_i.exp();
    // The forward diode's ln(1 + i/IS), as ln(i/IS) + ln(1 + y) with y = IS/i.
    // From 1 pA up y ≤ 0.01, so two series terms are exact to under a microvolt.
    let y = ZENER_IS / current;
    let forward_ln = ln_i - ZENER_IS.ln() + y * (1.0 - 0.5 * y);
    ZenerPoint {
        current,
        voltage: VT * forward_ln
            + ZENER_BV
            + VT * (ln_i - ZENER_IBV.ln())
            + 2.0 * ZENER_RS * current,
        slope: VT / (1.0 + y) + VT + 2.0 * ZENER_RS * current,
    }
}

/// Voltage across the pair carrying current `exp(ln_i)`.
pub(super) fn zener_pair_voltage(ln_i: f64) -> f64 {
    zener_pair(ln_i).voltage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zener_pair_knees_near_five_and_a_half_volts() {
        let at = |i: f64| zener_pair_voltage(i.ln());
        assert!((at(10.0e-6) - 5.48).abs() < 0.02);
        assert!((at(1.0e-3) - 5.75).abs() < 0.02);
    }

    #[test]
    fn voltage_matches_the_exact_diode_equations() {
        let mut ln_i = ZENER_MIN_LN_I;
        while ln_i < (30.0e-3_f64).ln() {
            let i = ln_i.exp();
            let exact = VT * (i / ZENER_IS).ln_1p()
                + ZENER_BV
                + VT * (i / ZENER_IBV).ln()
                + 2.0 * ZENER_RS * i;
            let v = zener_pair_voltage(ln_i);
            assert!((v - exact).abs() < 1.0e-6, "ln_i={ln_i}: {v} vs {exact}");
            ln_i += 0.25;
        }
    }

    #[test]
    fn slope_matches_finite_difference() {
        for ln_i in [-25.0, -12.0, -6.0, -3.0] {
            let h = 1.0e-6;
            let fd = (zener_pair_voltage(ln_i + h) - zener_pair_voltage(ln_i - h)) / (2.0 * h);
            assert!((fd - zener_pair(ln_i).slope).abs() < 1.0e-6, "ln_i={ln_i}");
        }
    }
}
