//! A linear network solved by modified nodal analysis, for the parts of the
//! 808's voices that are resistors, capacitors and ideal op-amps.
//!
//! Each capacitor is replaced by its trapezoidal companion: a conductance
//! 2C/dt in parallel with a current source carrying its history, which
//! discretises the network exactly as the bilinear transform would. Switched
//! elements (a transistor pulling a node to a fixed voltage, a diode
//! charging a capacitor) are conductances to a voltage that are in or out
//! of the network; the system matrix is inverted once per combination of
//! switch states, so a sample costs one matrix-vector product.

use super::lanes::{Lanes, Lanewise, Mask, Mono, Real, c};

/// Ground, as a node index.
pub const GROUND: usize = usize::MAX;

/// A switched conductance `g` from `node` to `other` (which may be
/// [`GROUND`]) in series with a source of `volts`, positive toward `node`.
#[derive(Clone, Copy)]
pub struct Switch {
    pub node: usize,
    pub other: usize,
    pub g: f32,
    pub volts: f32,
}

impl Default for Switch {
    fn default() -> Self {
        Self {
            node: GROUND,
            other: GROUND,
            g: 0.0,
            volts: 0.0,
        }
    }
}

/// The network's matrices and element list. `N` unknowns (node voltages,
/// then op-amp output currents), `C` capacitors, `S` switches and `K` =
/// 2^S inverse matrices.
#[derive(Clone)]
pub struct Mna<const N: usize, const C: usize, const S: usize, const K: usize> {
    /// Each inverse by column: `inverse[k][j]` is every unknown's response to
    /// a unit current into unknown `j`, so a solve sums whole columns, which
    /// vectorises.
    inverse: [[[f32; N]; N]; K],
    caps: [(usize, usize, f32); C],
    switches: [Switch; S],
}

impl<const N: usize, const C: usize, const S: usize, const K: usize> Default for Mna<N, C, S, K> {
    fn default() -> Self {
        Self {
            inverse: [[[0.0; N]; N]; K],
            caps: [(GROUND, GROUND, 0.0); C],
            switches: [Switch::default(); S],
        }
    }
}

/// The capacitors' trapezoidal history: voltage and current at the last
/// sample.
#[derive(Clone, Copy)]
pub struct MnaState<const C: usize, L: Lanes = Mono> {
    pub v: [L::S; C],
    pub i: [L::S; C],
}

impl<const C: usize, L: Lanes> Default for MnaState<C, L> {
    fn default() -> Self {
        Self {
            v: [c(0.0); C],
            i: [c(0.0); C],
        }
    }
}

impl<const C: usize, L: Lanes> Lanewise for MnaState<C, L> {
    type Lane = MnaState<C>;
    #[inline]
    fn lane(&self, lane: usize) -> MnaState<C> {
        MnaState {
            v: self.v.lane(lane),
            i: self.i.lane(lane),
        }
    }
    #[inline]
    fn set_lane(&mut self, lane: usize, value: &MnaState<C>) {
        self.v.set_lane(lane, &value.v);
        self.i.set_lane(lane, &value.i);
    }
}

/// Collects a network's elements before [`Mna`] discretises it.
pub struct MnaBuilder<const N: usize> {
    g: [[f64; N]; N],
    caps: Vec<(usize, usize, f64)>,
}

impl<const N: usize> Default for MnaBuilder<N> {
    fn default() -> Self {
        Self {
            g: [[0.0; N]; N],
            caps: Vec::new(),
        }
    }
}

impl<const N: usize> MnaBuilder<N> {
    fn stamp(g: &mut [[f64; N]; N], a: usize, b: usize, value: f64) {
        if a != GROUND {
            g[a][a] += value;
        }
        if b != GROUND {
            g[b][b] += value;
        }
        if a != GROUND && b != GROUND {
            g[a][b] -= value;
            g[b][a] -= value;
        }
    }

    pub fn resistor(&mut self, a: usize, b: usize, ohms: f32) -> &mut Self {
        Self::stamp(&mut self.g, a, b, 1.0 / ohms as f64);
        self
    }

    pub fn capacitor(&mut self, a: usize, b: usize, farads: f32) -> &mut Self {
        self.caps.push((a, b, farads as f64));
        self
    }

    /// An ideal op-amp holding `plus` and `minus` equal by driving `out`,
    /// with its output current as unknown `row`.
    pub fn opamp(&mut self, plus: usize, minus: usize, out: usize, row: usize) -> &mut Self {
        self.g[out][row] -= 1.0;
        if plus != GROUND {
            self.g[row][plus] += 1.0;
        }
        if minus != GROUND {
            self.g[row][minus] -= 1.0;
        }
        self
    }

    /// Discretises the network at time step `dt` with `switches`.
    pub fn build<const C: usize, const S: usize, const K: usize>(
        &self,
        dt: f32,
        switches: [Switch; S],
    ) -> Mna<N, C, S, K> {
        assert_eq!(self.caps.len(), C, "capacitor count");
        assert_eq!(1 << S, K, "one inverse per switch combination");
        let mut mna = Mna::<N, C, S, K>::default();
        let mut g = self.g;
        for (k, &(a, b, farads)) in self.caps.iter().enumerate() {
            let gc = 2.0 * farads / dt as f64;
            Self::stamp(&mut g, a, b, gc);
            mna.caps[k] = (a, b, gc as f32);
        }
        mna.switches = switches;
        for (combo, inverse) in mna.inverse.iter_mut().enumerate() {
            let mut m = g;
            for (s, sw) in switches.iter().enumerate() {
                if combo & (1 << s) != 0 {
                    Self::stamp(&mut m, sw.node, sw.other, sw.g as f64);
                }
            }
            *inverse = transpose(invert(m));
        }
        mna
    }
}

fn transpose<const N: usize>(m: [[f32; N]; N]) -> [[f32; N]; N] {
    std::array::from_fn(|j| std::array::from_fn(|i| m[i][j]))
}

/// Gauss-Jordan inverse with partial pivoting.
fn invert<const N: usize>(mut m: [[f64; N]; N]) -> [[f32; N]; N] {
    let mut inv = [[0.0f64; N]; N];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..N {
        let pivot = (col..N)
            .max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))
            .unwrap_or(col);
        m.swap(col, pivot);
        inv.swap(col, pivot);
        let p = m[col][col];
        for j in 0..N {
            m[col][j] /= p;
            inv[col][j] /= p;
        }
        for r in 0..N {
            if r != col {
                let f = m[r][col];
                if f != 0.0 {
                    for j in 0..N {
                        m[r][j] -= f * m[col][j];
                        inv[r][j] -= f * inv[col][j];
                    }
                }
            }
        }
    }
    inv.map(|row| row.map(|v| v as f32))
}

impl<const N: usize, const C: usize, const S: usize, const K: usize> Mna<N, C, S, K> {
    /// Solves one sample with switch `s` closed where bit `s` of `closed` is
    /// set, plus current `injected[n]` driven into each node `n`, returning
    /// every unknown and advancing the capacitors' history.
    #[inline]
    pub fn step<L: Lanes>(
        &self,
        state: &mut MnaState<C, L>,
        closed: L::Closed,
        injected: &[L::S; N],
    ) -> [L::S; N] {
        let x = self.open(state, closed, injected);
        self.close(state, closed, x, &[])
    }

    /// The sample's solution with no current into the port node yet: its
    /// voltage there is the port's open-circuit voltage.
    #[inline]
    pub fn open<L: Lanes>(
        &self,
        state: &MnaState<C, L>,
        closed: L::Closed,
        injected: &[L::S; N],
    ) -> [L::S; N] {
        let mut rhs = *injected;
        for (k, &(a, b, gc)) in self.caps.iter().enumerate() {
            let j = state.v[k] * gc + state.i[k];
            if a != GROUND {
                rhs[a] += j;
            }
            if b != GROUND {
                rhs[b] -= j;
            }
        }
        for (s, sw) in self.switches.iter().enumerate() {
            let on = L::switch_on(closed, s);
            if on.any() {
                let j = sw.g * sw.volts;
                if sw.node != GROUND {
                    rhs[sw.node] = L::S::select(on, rhs[sw.node] + j, rhs[sw.node]);
                }
                if sw.other != GROUND {
                    rhs[sw.other] = L::S::select(on, rhs[sw.other] - j, rhs[sw.other]);
                }
            }
        }
        let mut x = [c(0.0); N];
        match L::uniform(closed) {
            Some(k) => {
                for (column, &r) in self.inverse[k].iter().zip(rhs.iter()) {
                    add_column::<L, N>(&mut x, column, r);
                }
            }
            None => {
                for (j, &r) in rhs.iter().enumerate() {
                    self.add_gathered::<L>(&mut x, closed, j, r);
                }
            }
        }
        x
    }

    /// Adds `current` times column `j` of each lane's inverse for `closed`
    /// to `x`.
    #[inline]
    fn add_gathered<L: Lanes>(
        &self,
        x: &mut [L::S; N],
        closed: L::Closed,
        j: usize,
        current: L::S,
    ) {
        for (i, xi) in x.iter_mut().enumerate() {
            let m = L::gather(|lane| self.inverse[L::closed_lane(closed, lane)][j][i]);
            *xi = m.mul_add(current, *xi);
        }
    }

    /// How far node `a`'s voltage rises per ampere driven into node `b`.
    #[inline]
    pub fn impedance<L: Lanes>(&self, closed: L::Closed, a: usize, b: usize) -> L::S {
        match L::uniform(closed) {
            Some(k) => L::S::splat(self.inverse[k][b][a]),
            None => L::gather(|lane| self.inverse[L::closed_lane(closed, lane)][b][a]),
        }
    }

    /// Completes the sample from [`Self::open`]'s solution `x` with each
    /// `(node, current)` driven into its node, returning every unknown and
    /// advancing the capacitors' history.
    #[inline]
    pub fn close<L: Lanes>(
        &self,
        state: &mut MnaState<C, L>,
        closed: L::Closed,
        mut x: [L::S; N],
        ports: &[(usize, L::S)],
    ) -> [L::S; N] {
        for &(node_index, current) in ports {
            match L::uniform(closed) {
                Some(k) => add_column::<L, N>(&mut x, &self.inverse[k][node_index], current),
                None => self.add_gathered::<L>(&mut x, closed, node_index, current),
            }
        }
        for (k, &(a, b, gc)) in self.caps.iter().enumerate() {
            let v = node::<L, N>(&x, a) - node::<L, N>(&x, b);
            let j = state.v[k] * gc + state.i[k];
            state.i[k] = v * gc - j;
            state.v[k] = v;
        }
        x
    }
}

/// Adds `current` times `column` to `x`.
#[inline(always)]
fn add_column<L: Lanes, const N: usize>(x: &mut [L::S; N], column: &[f32; N], current: L::S) {
    for (xi, &m) in x.iter_mut().zip(column.iter()) {
        *xi = L::S::splat(m).mul_add(current, *xi);
    }
}

#[inline]
fn node<L: Lanes, const N: usize>(x: &[L::S; N], n: usize) -> L::S {
    if n == GROUND { c(0.0) } else { x[n] }
}
