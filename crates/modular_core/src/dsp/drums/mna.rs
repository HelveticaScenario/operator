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
pub struct MnaState<const C: usize> {
    pub v: [f32; C],
    pub i: [f32; C],
}

impl<const C: usize> Default for MnaState<C> {
    fn default() -> Self {
        Self {
            v: [0.0; C],
            i: [0.0; C],
        }
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
            *inverse = invert(m);
        }
        mna
    }
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
    pub fn step(&self, state: &mut MnaState<C>, closed: usize, injected: &[f32; N]) -> [f32; N] {
        let x = self.open(state, closed, injected);
        self.close(state, closed, x, &[])
    }

    /// The sample's solution with no current into the port node yet: its
    /// voltage there is the port's open-circuit voltage.
    #[inline]
    pub fn open(&self, state: &MnaState<C>, closed: usize, injected: &[f32; N]) -> [f32; N] {
        let mut rhs = *injected;
        for (k, &(a, b, gc)) in self.caps.iter().enumerate() {
            let j = gc * state.v[k] + state.i[k];
            if a != GROUND {
                rhs[a] += j;
            }
            if b != GROUND {
                rhs[b] -= j;
            }
        }
        for (s, sw) in self.switches.iter().enumerate() {
            if closed & (1 << s) != 0 {
                let j = sw.g * sw.volts;
                if sw.node != GROUND {
                    rhs[sw.node] += j;
                }
                if sw.other != GROUND {
                    rhs[sw.other] -= j;
                }
            }
        }
        let inverse = &self.inverse[closed];
        let mut x = [0.0f32; N];
        for (xi, row) in x.iter_mut().zip(inverse.iter()) {
            *xi = row.iter().zip(rhs.iter()).map(|(m, r)| m * r).sum();
        }
        x
    }

    /// How far node `a`'s voltage rises per ampere driven into node `b`.
    #[inline]
    pub fn impedance(&self, closed: usize, a: usize, b: usize) -> f32 {
        self.inverse[closed][a][b]
    }

    /// Completes the sample from [`Self::open`]'s solution `x` with each
    /// `(node, current)` driven into its node, returning every unknown and
    /// advancing the capacitors' history.
    #[inline]
    pub fn close(
        &self,
        state: &mut MnaState<C>,
        closed: usize,
        mut x: [f32; N],
        ports: &[(usize, f32)],
    ) -> [f32; N] {
        let inverse = &self.inverse[closed];
        for &(node_index, current) in ports {
            for (xi, row) in x.iter_mut().zip(inverse.iter()) {
                *xi += row[node_index] * current;
            }
        }
        for (k, &(a, b, gc)) in self.caps.iter().enumerate() {
            let v = node(&x, a) - node(&x, b);
            let j = gc * state.v[k] + state.i[k];
            state.i[k] = gc * v - j;
            state.v[k] = v;
        }
        x
    }
}

#[inline]
fn node<const N: usize>(x: &[f32; N], n: usize) -> f32 {
    if n == GROUND { 0.0 } else { x[n] }
}
