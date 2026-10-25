use super::*;

/// The open hat's circuit: the trigger pulse, C60's timer, C62's envelope,
/// Q27's VCA and the output stage.
#[derive(Clone, Copy, Default)]
pub struct Open<L: Lanes = Mono> {
    pulse: Pulse<L>,
    /// C60's voltage and Q22's base-emitter voltage.
    c60: L::S,
    q22_vbe: Junction<L>,
    envelope: MnaState<1, L>,
    e62: L::S,
    /// Q29's base current, drawn from C62.
    q29: L::S,
    net: MnaState<OH_CAPS, L>,
    base: SwingState<L>,
    node: L::S,
    primed: L::SM,
    quiet: Quiet<L>,
}

lanewise!(Open {
    pulse,
    c60,
    q22_vbe,
    envelope,
    e62,
    q29,
    net,
    base,
    node,
    primed,
    quiet,
});

/// Settings read once per sample.
#[derive(Clone, Copy, Default)]
pub struct OpenControls<L: Lanes = Mono> {
    pub band: L::S,
    /// Conductance from the rail to C60: the decay pot and R132.
    pub decay: L::S,
    /// Q23's base current, from the closed hat's envelope, and the
    /// resistance C62 drains through while Q23 conducts.
    pub q23: L::S,
    pub drain: L::S,
}

lanewise!(OpenControls {
    band,
    decay,
    q23,
    drain
});

impl<L: Lanes> Open<L> {
    /// Advances C60 one sample by backward Euler, with Q21 emptying it while
    /// `striking`, and returns Q22's base current.
    #[inline(always)]
    fn timer(&mut self, dt: f32, decay: L::S, striking: L::SM) -> L::S {
        // C60 integrates this solve's error every sample, so it settles far
        // tighter than the junctions that feed no integrator.
        const DONE: f64 = converged(1.0e-12, VT as f64);
        let (vt, inv_vt, is_b) = (VT as f64, 1.0 / VT as f64, IS / BETA as f64);
        let (c_dt, g, r) = ((C60 / dt) as f64, L::wide(decay), R133 as f64);
        let g21 = L::D::select(L::wide_mask(striking), c(1.0 / R_Q21 as f64), c(0.0));
        let old = L::wide(self.c60);
        let rail = RAIL as f64;
        // Newton on Q22's base-emitter voltage; C60 sits R133·Ib above it.
        let mut vbe = self.q22_vbe.v;
        let mut e = self.q22_vbe.e.at(vbe, inv_vt);
        let mut v = old;
        let mut ib = c::<L::D>(0.0);
        let mut done = L::DM::default();
        for _ in 0..30 {
            let ib_now = (e.value - 1.0) * is_b;
            let dib = e.value * is_b * inv_vt;
            let v_now = vbe + ib_now * r;
            let dv = dib * r + 1.0;
            let f = (v_now - old) * c_dt - (c::<L::D>(rail) - v_now) * g + ib_now + v_now * g21;
            let df = (g + c_dt + g21) * dv + dib;
            let next = vbe - f / df;
            let limited = vbe + (next - vbe).clamp(c(-0.4), c(2.0 * vt));
            let next = L::D::select(next.gt(c(0.5)), limited, next);
            let moved = next - vbe;
            vbe = L::D::select(done, vbe, next);
            v = L::D::select(done, v, v_now);
            ib = L::D::select(done, ib, ib_now);
            done = done | (moved * moved).lt(c(DONE));
            if done.all() {
                break;
            }
            e = e.at_where(!done, vbe, inv_vt);
        }
        self.q22_vbe = Junction { v: vbe, e };
        self.c60 = L::narrow(v);
        L::narrow(ib)
    }
}

/// Brings a fully accented hit to twice the level of an unaccented one.
const ACCENT_TRIM: f32 = 0.945;

impl Fire for Open {
    fn accent_trim(_: &HatShared) -> f32 {
        ACCENT_TRIM
    }

    fn fire(&mut self, accent: f32, shared: &HatShared) {
        let volts = pulse_volts(accent);
        if !self.primed {
            self.base = shared.rest;
            self.primed = true;
        }
        self.pulse.fire(volts, shared.timing.trigger_samples);
    }
}

impl<L: Lanes> Circuit<L> for Open<L> {
    type Shared = HatShared;
    type Controls = OpenControls<L>;

    fn settled(&self) -> L::SM {
        !self.pulse.active() & self.quiet.below(SETTLED)
    }

    #[inline]
    fn step(&mut self, sh: &HatShared, ctl: &OpenControls<L>) -> L::S {
        let vtrig = self.pulse.next();
        let striking = vtrig.gt(c(0.0));
        let q22 = self.timer(sh.dt, ctl.decay, striking);

        let mut charge = L::SM::default();
        let mut injected = [c(0.0); 1];
        if striking.any() {
            let target = charge_target(vtrig);
            charge = striking & target.gt(self.e62);
            injected[E62] = L::S::select(charge, target / R_Q24, c(0.0));
        }
        injected[E62] -= self.q29;
        let closed = L::closed([charge]);
        let x = sh.envelope.open(&self.envelope, closed, &injected);
        // Q22 and Q23 drain C62 through R134, or Q23's own path while it
        // conducts, as hard as their base currents let them, until they
        // saturate.
        let z = sh.envelope.impedance::<L>(closed, E62, E62);
        let demand = (q22 + ctl.q23) * BETA;
        let r = L::S::select(ctl.q23.gt(c(0.0)), ctl.drain, c(R134));
        let available = ((x[E62] - DRAIN_SAT) / (z + r)).max(c(0.0));
        let drain = demand.min(available);
        let x = sh
            .envelope
            .close(&mut self.envelope, closed, x, &[(E62, -drain)]);
        self.e62 = x[E62];

        // Q29 follows C62 into the VCA node through R144.
        let e29 = (self.e62 - Q29_VBE).max(c(0.0));
        let mut injected = [c(0.0); OH_UNKNOWNS];
        injected[OH_NODE] = e29 / R144;
        let x = vca(
            &sh.open,
            &mut self.net,
            L::closed([]),
            OH_NODE,
            &mut self.base,
            ctl.band,
            &injected,
            sh.dt,
        );
        self.node = x[OH_NODE];
        let following = e29.gt(c(0.0));
        self.q29 = if following.any() {
            let q29 = (e29 / R143 + (e29 - self.node) / R144) / (BETA + 1.0);
            L::S::select(following, q29, c(0.0))
        } else {
            c(0.0)
        };
        let out = x[OH_OUT];
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
