use super::*;

/// One triangle VCO whose two slopes may differ.
#[derive(Clone, Copy, Default)]
pub(super) struct Vco<L: Lanes = Mono> {
    pub(super) tri: L::S,
    pub(super) rising: L::SM,
}

lanewise!(Vco { tri, rising });

impl<L: Lanes> Vco<L> {
    /// Advances `dt` rising at `up` V/s and falling at `down` V/s,
    /// reflecting off the comparator's thresholds.
    #[inline]
    pub(super) fn run(&mut self, up: L::S, down: L::S, dt: f32) {
        let mut left = c::<L::S>(dt);
        let mut active = left.gt(c(0.0));
        while active.any() {
            let rate = L::S::select(self.rising, up, down);
            let room = L::S::select(
                self.rising,
                c::<L::S>(THRESHOLD) - self.tri,
                self.tri + THRESHOLD,
            );
            let need = room.max(c(0.0)) / rate;
            let finish = active & need.gt(left);
            let moved = self.tri + L::S::select(self.rising, rate * left, -rate * left);
            let reflect = active & !finish;
            let edge = L::S::select(self.rising, c(THRESHOLD), c(-THRESHOLD));
            self.tri = L::S::select(finish, moved, L::S::select(reflect, edge, self.tri));
            left = L::S::select(reflect, left - need, left);
            self.rising = (reflect & !self.rising) | (!reflect & self.rising);
            active = reflect & left.gt(c(0.0));
        }
    }

    /// `a` in the lanes of `mask`, `b` in the others.
    #[inline]
    pub(super) fn select(mask: L::SM, a: Self, b: Self) -> Self {
        Self {
            tri: L::S::select(mask, a.tri, b.tri),
            rising: (mask & a.rising) | (!mask & b.rising),
        }
    }
}

/// Q18's clipper settles to 1e-7 V on every node.
const CLIPPER_DONE: f64 = converged(1.0e-7, VT as f64);

/// Q18's clipper: its collector, its emitter with the exponential of the
/// voltage between them, and the diode node.
#[derive(Clone, Copy, Default)]
pub(super) struct Clipper<L: Lanes = Mono> {
    collector: L::D,
    emitter: L::D,
    ce: Exp<L>,
    diodes: Junction<L>,
}

lanewise!(Clipper {
    collector,
    emitter,
    ce,
    diodes
});

impl<L: Lanes> Clipper<L> {
    /// Solves VCO-1 at `vi` through R102 into Q18's collector, its emitter
    /// through R103 onto the diodes, and R104 on to Q19's collector near
    /// ground, with Q18's base current `ib`, in the lanes of `on`; the
    /// others keep their nodes. Returns the current into R104.
    #[inline(always)]
    pub(super) fn solve(&mut self, on: L::DM, vi: L::D, ib: L::D) -> L::D {
        const G102: f64 = 1.0 / R102;
        const G103: f64 = 1.0 / R103;
        const G104: f64 = 1.0 / R104;
        let (mut va, mut vb, mut vd) = (self.collector, self.emitter, self.diodes.v);
        let (mut k, mut e) = (self.ce.at(va - vb, COLLECTOR), self.diodes.e.at(vd, DIODE));
        let mut done = !on;
        for _ in 0..30 {
            let q = VCA.currents(ib, k);
            let (id, did) = diode_pair(e);
            let f = [
                (vi - va) * G102 - q.collector,
                q.emitter - (vb - vd) * G103,
                (vb - vd) * G103 - id - vd * G104,
            ];
            let j = [
                [c::<L::D>(-G102) - q.slope, q.slope, c(0.0)],
                [q.slope, -q.slope - G103, c(G103)],
                [c(0.0), c(G103), c::<L::D>(-G103) - did - G104],
            ];
            let s = solve3::<L>(j, f);
            va = L::D::select(done, va, va - s[0].clamp(c(-1.0), c(1.0)));
            vb = L::D::select(done, vb, vb - s[1].clamp(c(-1.0), c(1.0)));
            vd = L::D::select(done, vd, vd - s[2].clamp(c(-0.1), c(0.1)));
            let converged = s.iter().fold(!L::DM::default(), |all, v| {
                all & (*v * *v).lt(c(CLIPPER_DONE))
            });
            done = done | converged;
            if done.all() {
                break;
            }
            k = k.at_where(!done, va - vb, COLLECTOR);
            e = e.at_where(!done, vd, DIODE);
        }
        *self = Self {
            collector: va,
            emitter: vb,
            ce: Exp::select(on, k, self.ce),
            diodes: Junction {
                v: vd,
                e: Exp::select(on, e, self.diodes.e),
            },
        };
        vd * G104
    }
}

/// Solves the 3×3 system `j·x = f` by Cramer's rule.
#[inline]
fn solve3<L: Lanes>(j: [[L::D; 3]; 3], f: [L::D; 3]) -> [L::D; 3] {
    let det = |m: [[L::D; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let inv = c::<L::D>(1.0) / det(j);
    let mut x = [c(0.0); 3];
    for (k, xk) in x.iter_mut().enumerate() {
        let mut m = j;
        for (row, &fv) in m.iter_mut().zip(f.iter()) {
            row[k] = fv;
        }
        *xk = det(m) * inv;
    }
    x
}
