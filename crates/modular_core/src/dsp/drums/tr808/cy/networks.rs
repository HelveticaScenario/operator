use super::*;

pub(super) fn envelope_network(dt: f32) -> Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS> {
    let mut b = MnaBuilder::<ENV_UNKNOWNS>::default();
    b.capacitor(E1, GROUND, 1.0e-6)
        .resistor(E1, E37, 22.0e3)
        .capacitor(E37, GROUND, 2.2e-6)
        .resistor(E1, GROUND, R94)
        .capacitor(E2A, GROUND, 1.0e-6)
        .resistor(E2A, E2, 33.0e3)
        .capacitor(E2, GROUND, 0.47e-6)
        .resistor(E2, GROUND, R90)
        .capacitor(B41, GROUND, 1.0e-6)
        .resistor(E20, J, 33.0e3)
        .resistor(J, GROUND, 10.0e3)
        .resistor(J, GROUND, R91)
        .resistor(E20, E3, 33.0e3)
        .capacitor(E3, GROUND, 2.2e-6)
        .resistor(E3, GROUND, R104);
    let charge = |node| Switch {
        node,
        other: GROUND,
        g: 1.0 / R_CHARGE,
        volts: 0.0,
    };
    b.build(
        dt,
        [
            charge(E1),
            charge(E2A),
            charge(B41),
            Switch {
                node: E20,
                other: GROUND,
                g: 1.0 / R_Q20,
                volts: 0.0,
            },
        ],
    )
}

/// Q16's node: C36 and C47 (the diode joins them while the VCA pulls the
/// node down), R94 from its envelope, and the third-order high-pass on
/// IC5a.
pub(super) fn high_network(dt: f32) -> Mna<HIGH_UNKNOWNS, HIGH_CAPS, 0, 1> {
    let (h1, h2, h3, m5a, out, opamp) = (1, 2, 3, 4, HIGH_OUT, 6);
    let mut b = MnaBuilder::<HIGH_UNKNOWNS>::default();
    b.capacitor(NODE, GROUND, 0.0015e-6)
        .capacitor(NODE, GROUND, 0.0015e-6)
        .resistor(NODE, GROUND, R94)
        .capacitor(NODE, h1, 0.0033e-6)
        .resistor(h1, GROUND, 6.8e3)
        .capacitor(h1, h2, 0.001e-6)
        .resistor(h2, out, 18.0e3)
        .capacitor(h2, h3, 0.001e-6)
        .resistor(h3, GROUND, 18.0e3)
        .resistor(m5a, GROUND, 22.0e3)
        .resistor(m5a, out, 39.0e3)
        .opamp(h3, m5a, out, opamp);
    b.build(dt, [])
}

/// Q17's node: C43 and C50, R90 from its envelope and R91 from Q20, and the
/// high-pass on IC5b.
pub(super) fn mid_network(dt: f32) -> Mna<MID_UNKNOWNS, MID_CAPS, 0, 1> {
    let (g1, g2, m5b, out, opamp) = (1, 2, 3, MID_OUT, 5);
    let mut b = MnaBuilder::<MID_UNKNOWNS>::default();
    b.capacitor(NODE, GROUND, 0.003e-6)
        .resistor(NODE, GROUND, R90)
        .resistor(NODE, GROUND, R91)
        .capacitor(NODE, g1, 0.001e-6)
        .capacitor(g1, g2, 0.001e-6)
        .resistor(g2, GROUND, 18.0e3)
        .resistor(g1, out, 18.0e3)
        .resistor(m5b, GROUND, 39.0e3)
        .resistor(m5b, out, 39.0e3)
        .opamp(g2, m5b, out, opamp);
    b.build(dt, [])
}

/// Q18's node: R104 from its envelope, and Q25's Sallen-Key high-pass with
/// Q25 as a unity follower.
pub(super) fn low_network(dt: f32) -> Mna<LOW_UNKNOWNS, LOW_CAPS, 0, 1> {
    let (f1, f2, out, follower) = (1, 2, LOW_OUT, 4);
    let mut b = MnaBuilder::<LOW_UNKNOWNS>::default();
    b.resistor(NODE, GROUND, R104)
        .capacitor(NODE, f1, 0.0015e-6)
        .capacitor(f1, f2, 0.0015e-6)
        .resistor(f1, out, 22.0e3)
        .resistor(f2, GROUND, 82.0e3)
        .opamp(f2, out, out, follower);
    b.build(dt, [])
}

/// The tone stage and IC6, with the level pot at maximum. The tone pot's
/// halves are run-time shunts at `TT` and `V125`.
pub(super) fn tone_network(dt: f32) -> Mna<TONE_UNKNOWNS, TONE_CAPS, 0, 1> {
    let mut b = MnaBuilder::<TONE_UNKNOWNS>::default();
    b.resistor(T_A, GROUND, 1.0 / STIFF)
        .resistor(T_B, GROUND, 1.0 / STIFF)
        .resistor(T_C, GROUND, 1.0 / STIFF)
        .capacitor(T_A, T55, 0.01e-6)
        .resistor(T55, TT, 22.0e3)
        .resistor(V125, VV, 2.2e3)
        .capacitor(T_B, T56, 0.01e-6)
        .resistor(T56, VV, 10.0e3)
        .capacitor(T_C, T58, 0.01e-6)
        .resistor(T58, F3, 100.0e3)
        .capacitor(F3, GROUND, 0.0033e-6)
        .resistor(F3, VV, 10.0e3)
        .resistor(TT, W, 22.0e3)
        .resistor(VV, W, 15.0e3)
        .capacitor(W, M6, 0.01e-6)
        .resistor(M6, TONE_OUT, 82.5e3)
        .capacitor(M6, TONE_OUT, 220.0e-12)
        .opamp(GROUND, M6, TONE_OUT, TONE_OPAMP);
    b.build(dt, [])
}
