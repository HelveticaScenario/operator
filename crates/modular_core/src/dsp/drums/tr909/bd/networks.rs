use super::*;

pub(super) fn envelope_network(dt: f32) -> Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS> {
    let mut b = MnaBuilder::<ENV_UNKNOWNS>::default();
    b.resistor(DV, X, 1.8e3)
        .resistor(DV, Y, 47.0e3)
        .resistor(Y, GROUND, 10.0e3)
        .capacitor(E1, GROUND, 0.33e-6);
    let diode = |from, to| Switch {
        node: to,
        other: from,
        g: 1.0 / R_DIODE,
        volts: -DIODE_DROP,
    };
    b.build(
        dt,
        [
            Switch {
                node: DV,
                other: GROUND,
                g: 1.0 / R_SWITCH,
                volts: 0.0,
            },
            diode(X, Y),
            diode(X, E1),
        ],
    )
}

pub(super) fn attack_network(
    dt: f32,
) -> Mna<ATTACK_UNKNOWNS, ATTACK_CAPS, ATTACK_SWITCHES, ATTACK_COMBOS> {
    let mut b = MnaBuilder::<ATTACK_UNKNOWNS>::default();
    b.resistor(NOISE_IN, GROUND, 1.0 / STIFF)
        .resistor(NOISE_IN, N1, 12.0e3)
        .resistor(N1, GROUND, 4.7e3)
        .capacitor(N1, N2, 0.47e-6)
        .resistor(N2, N3, 4.7e3)
        .capacitor(N3, GROUND, 0.1e-6)
        .resistor(N3, NB, 4.7e3)
        .resistor(NB, GROUND, 330.0)
        // Q8's collector: C5 and R14, then C2 and R7 into the Sallen-Key
        // low-pass on Q9 (R15, R16, C3, C4), R8 on its emitter, and C11
        // and R44 into NB.
        .capacitor(P0, GROUND, 0.0068e-6)
        .resistor(P0, GROUND, 22.0e3)
        .capacitor(P0, P1, 0.0033e-6)
        .resistor(P1, PA, 100.0e3)
        .resistor(PA, GROUND, 100.0e3)
        .resistor(PA, PB, 39.0e3)
        .capacitor(PB, GROUND, 100.0e-12)
        .capacitor(PA, PE, 0.0047e-6)
        .opamp(PB, PE, PE, OP9)
        .resistor(PE, GROUND, 10.0e3)
        .capacitor(PE, P4, 0.47e-6)
        .resistor(P4, NB, 22.0e3);
    b.build(
        dt,
        [Switch {
            node: P0,
            other: GROUND,
            g: 1.0 / 100.0,
            volts: RAIL,
        }],
    )
}
