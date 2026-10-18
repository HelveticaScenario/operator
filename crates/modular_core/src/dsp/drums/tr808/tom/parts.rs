/// Component values that differ between the six voices.
pub(super) struct Parts {
    /// Input coupling capacitor (C73) and the divider it drives: the series
    /// resistance to IC15's input and the shunt to ground, with the TOM/CONGA
    /// switch's 1.5–2.2 kΩ resistor already placed.
    pub(super) c_in: f32,
    pub(super) r_series: f32,
    pub(super) r_shunt: f32,
    /// Bridged-T arms (C76 and C78 with any switched capacitor).
    pub(super) c_a: f32,
    pub(super) c_b: f32,
    /// Output coupling capacitor (C81).
    pub(super) c_out: f32,
    pub(super) noise: Option<NoiseParts>,
    /// Scales the output so an unaccented hit at stock pitch peaks near 5 V.
    pub(super) gain: f32,
}

/// The tom's noise gate capacitors: C72, C71, C74 and C75.
pub(super) struct NoiseParts {
    pub(super) c_env: f32,
    pub(super) c_couple: f32,
    pub(super) c_hp: f32,
    pub(super) c_lp: f32,
}

pub(super) const fn parallel(a: f32, b: f32) -> f32 {
    a * b / (a + b)
}

pub(super) static LT: Parts = Parts {
    c_in: 6.8e-9,
    r_series: parallel(22.0e3, 1.5e3),
    r_shunt: 4.7e3,
    c_a: 0.056e-6,
    c_b: 0.012e-6 + 0.047e-6,
    c_out: 0.047e-6,
    noise: Some(NoiseParts {
        c_env: 0.039e-6,
        c_couple: 0.047e-6,
        c_hp: 0.047e-6,
        c_lp: 0.018e-6,
    }),
    gain: 1.859,
};

pub(super) static LC: Parts = Parts {
    c_in: 6.8e-9,
    r_series: 22.0e3,
    r_shunt: parallel(4.7e3, 1.5e3),
    c_a: 0.056e-6,
    c_b: 0.012e-6,
    c_out: 0.047e-6,
    noise: None,
    gain: 3.356,
};

pub(super) static MT: Parts = Parts {
    c_in: 2.7e-9,
    r_series: parallel(22.0e3, 2.2e3),
    r_shunt: 6.8e3,
    c_a: 0.027e-6 + 0.0082e-6,
    c_b: 0.012e-6 + 0.027e-6,
    c_out: 0.039e-6,
    noise: Some(NoiseParts {
        c_env: 0.033e-6,
        c_couple: 0.047e-6,
        c_hp: 0.033e-6,
        c_lp: 0.015e-6,
    }),
    gain: 2.203,
};

pub(super) static MC: Parts = Parts {
    c_in: 2.7e-9,
    r_series: 22.0e3,
    r_shunt: parallel(6.8e3, 2.2e3),
    c_a: 0.027e-6,
    c_b: 0.012e-6,
    c_out: 0.039e-6,
    noise: None,
    gain: 5.102,
};

pub(super) static HT: Parts = Parts {
    c_in: 2.7e-9,
    r_series: parallel(22.0e3, 1.5e3),
    r_shunt: 5.6e3,
    c_a: 0.027e-6,
    c_b: 0.0056e-6 + 0.022e-6,
    c_out: 0.033e-6,
    noise: Some(NoiseParts {
        c_env: 0.033e-6,
        c_couple: 0.047e-6,
        c_hp: 0.027e-6,
        c_lp: 0.01e-6,
    }),
    gain: 1.938,
};

pub(super) static HC: Parts = Parts {
    c_in: 2.7e-9,
    r_series: 22.0e3,
    r_shunt: parallel(5.6e3, 1.5e3),
    c_a: 0.027e-6,
    c_b: 0.0056e-6,
    c_out: 0.033e-6,
    noise: None,
    gain: 3.125,
};
