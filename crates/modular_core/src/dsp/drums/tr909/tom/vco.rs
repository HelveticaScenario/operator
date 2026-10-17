use super::*;

/// One triangle VCO whose two slopes may differ.
#[derive(Clone, Copy, Default)]
pub(super) struct Vco {
    pub(super) tri: f32,
    pub(super) rising: bool,
}

impl Vco {
    /// Advances `dt` rising at `up` V/s and falling at `down` V/s,
    /// reflecting off the comparator's thresholds.
    #[inline]
    pub(super) fn run(&mut self, up: f32, down: f32, dt: f32) {
        let mut left = dt;
        while left > 0.0 {
            let (rate, room) = if self.rising {
                (up, THRESHOLD - self.tri)
            } else {
                (down, self.tri + THRESHOLD)
            };
            let need = room.max(0.0) / rate;
            if need > left {
                self.tri += if self.rising {
                    rate * left
                } else {
                    -rate * left
                };
                break;
            }
            self.tri = if self.rising { THRESHOLD } else { -THRESHOLD };
            left -= need;
            self.rising = !self.rising;
        }
    }
}

/// Solves Q18's clipper: VCO-1 at `vi` through R102 into Q18's collector
/// `a`, its emitter `b` through R103 onto the diodes at `d`, and R104 on to
/// Q19's collector near ground. Returns the current into R104.
#[inline]
pub(super) fn clipper(a: &mut f64, b: &mut f64, d: &mut f64, vi: f64, ib: f64) -> f64 {
    let (mut va, mut vb, mut vd) = (*a, *b, *d);
    for _ in 0..30 {
        let q = VCA.currents(ib, va - vb);
        let e = (vd / DIODE_NVT).clamp(-80.0, 80.0).exp();
        let id = DIODE_IS * (e - 1.0 / e);
        let did = DIODE_IS * (e + 1.0 / e) / DIODE_NVT;
        let f = [
            (vi - va) / R102 - q.collector,
            q.emitter - (vb - vd) / R103,
            (vb - vd) / R103 - id - vd / R104,
        ];
        let j = [
            [-1.0 / R102 - q.slope, q.slope, 0.0],
            [q.slope, -q.slope - 1.0 / R103, 1.0 / R103],
            [0.0, 1.0 / R103, -1.0 / R103 - did - 1.0 / R104],
        ];
        let s = solve3(j, f);
        va -= s[0].clamp(-1.0, 1.0);
        vb -= s[1].clamp(-1.0, 1.0);
        vd -= s[2].clamp(-0.1, 0.1);
        if s.iter().all(|v| v.abs() < 1.0e-7) {
            break;
        }
    }
    *a = va;
    *b = vb;
    *d = vd;
    vd / R104
}

/// Solves the 3×3 system `j·x = f` by Cramer's rule.
#[inline]
fn solve3(j: [[f64; 3]; 3], f: [f64; 3]) -> [f64; 3] {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(j);
    let mut x = [0.0; 3];
    for (k, xk) in x.iter_mut().enumerate() {
        let mut m = j;
        for (row, &fv) in m.iter_mut().zip(f.iter()) {
            row[k] = fv;
        }
        *xk = det(m) / d;
    }
    x
}

/// VCO-3's shaper into Q21 with the noise burst joining the collector node
/// through [`R194`].
#[inline]
pub(super) fn shaped_with_burst(d: &mut f64, vc: &mut f64, vi: f64, burst: f64, ib: f64) -> f64 {
    let (mut dn, mut cn) = (*d, *vc);
    let s = SHAPER;
    for _ in 0..20 {
        let e = (dn / DIODE_NVT).clamp(-80.0, 80.0).exp();
        let id = DIODE_IS * (e - 1.0 / e);
        let did = DIODE_IS * (e + 1.0 / e) / DIODE_NVT;
        let q = VCA.currents(ib, cn);
        let f1 = (vi - dn) / s.r_in - id - (dn - cn) / s.r_out;
        let f2 = (dn - cn) / s.r_out + (burst - cn) / R194 - cn / s.r_load - q.collector;
        let (a11, a12) = (-1.0 / s.r_in - did - 1.0 / s.r_out, 1.0 / s.r_out);
        let (a21, a22) = (
            1.0 / s.r_out,
            -1.0 / s.r_out - 1.0 / R194 - 1.0 / s.r_load - q.slope,
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
