//! The tone source the 808's cymbal and hi-hats share: six Schmitt-trigger
//! square-wave oscillators (one HD14584) summed through 120 kΩ resistors
//! into R53, and two band-pass filters on IC3 that pick out an upper band
//! near 6.9 kHz and a lower one near 3.4 kHz. Oscillator frequencies are
//! Werner, Abel and Smith's ("The TR-808 Cymbal: a Physically-Informed,
//! Circuit-Bendable, Digital Model", ICMC/SMC 2014), with the two cowbell
//! oscillators at their factory tuning.

use super::circuit::{ACCENT_V, PULSE_V};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::utils::rng::LcgRng;

// The metal voices' trigger buffer, IC2a, puts out 0.824·V + 3.29 for a
// common trigger pulse of V, 7–14 V over the 808's accent range. The
// unaccented 4 V pulse gives 6.6 V; full accent here spans the buffer's
// whole range to its 13.5 V limit, doubling the level as accent does on
// the other voices.
const METAL_REST: f32 = 0.824 * PULSE_V + 3.29;
const METAL_ACCENT: f32 = 13.5;

/// The trigger buffer's output for a common trigger pulse of `volts`.
pub fn metal_pulse(volts: f32) -> f32 {
    let accent = (volts - PULSE_V) / ACCENT_V;
    METAL_REST + (METAL_ACCENT - METAL_REST) * accent
}

/// The oscillators' frequencies, Hz.
const OSC_HZ: [f32; 6] = [205.3, 369.6, 304.4, 522.7, 800.0, 540.0];
/// Share of each period an oscillator spends high: the HD14584's thresholds
/// at 5 V give 47.98%.
const DUTY: f32 = 0.4798;
const OSC_VOLTS: f32 = 5.0;
/// Each oscillator's mixing resistor, and R53 to ground.
const R_MIX: f32 = 120.0e3;
const R53: f32 = 1.0e3;

// Network unknowns: the oscillator sum, then for each band the junction of
// its input capacitor and resistor, its op-amp's inverting input, its
// bridged-T junction and its output; then the op-amps' output currents.
const SUM: usize = 0;
const A_HIGH: usize = 1;
const INV_HIGH: usize = 2;
const T_HIGH: usize = 3;
pub const BAND_HIGH: usize = 4;
const A_LOW: usize = 5;
const INV_LOW: usize = 6;
const T_LOW: usize = 7;
pub const BAND_LOW: usize = 8;
const OPAMP_HIGH: usize = 9;
const OPAMP_LOW: usize = 10;
const UNKNOWNS: usize = 11;
const CAPS: usize = 6;

/// The band-pass network discretised for one sample rate.
#[derive(Default)]
pub struct MetalFilters {
    net: Mna<UNKNOWNS, CAPS, 0, 1>,
    /// Each oscillator's phase advance per sample.
    inc: [f32; 6],
}

impl MetalFilters {
    pub fn new(sample_rate: f32) -> Self {
        let mut b = MnaBuilder::<UNKNOWNS>::default();
        // The six mixing resistors, their sources taken as injected
        // currents, in parallel with R53.
        b.resistor(SUM, GROUND, 1.0 / (6.0 / R_MIX + 1.0 / R53))
            // IC3b: C11 and R55 into the bridged-T C15, C16, R58 with R59.
            .capacitor(SUM, A_HIGH, 0.001e-6)
            .resistor(A_HIGH, INV_HIGH, 22.0e3)
            .capacitor(INV_HIGH, T_HIGH, 0.0033e-6)
            .capacitor(T_HIGH, BAND_HIGH, 0.0033e-6)
            .resistor(T_HIGH, GROUND, 560.0)
            .resistor(INV_HIGH, BAND_HIGH, 82.0e3)
            .opamp(GROUND, INV_HIGH, BAND_HIGH, OPAMP_HIGH)
            // IC3a: C10 and R52 into C13, C14, R56 with R57.
            .capacitor(SUM, A_LOW, 0.0033e-6)
            .resistor(A_LOW, INV_LOW, 33.0e3)
            .capacitor(INV_LOW, T_LOW, 0.0068e-6)
            .capacitor(T_LOW, BAND_LOW, 0.0068e-6)
            .resistor(T_LOW, GROUND, 560.0)
            .resistor(INV_LOW, BAND_LOW, 82.0e3)
            .opamp(GROUND, INV_LOW, BAND_LOW, OPAMP_LOW);
        Self {
            net: b.build(1.0 / sample_rate, []),
            inc: OSC_HZ.map(|hz| hz / sample_rate),
        }
    }
}

/// One channel's oscillators and filters.
#[derive(Clone, Copy, Default)]
pub struct MetalBank {
    phases: [f32; 6],
    net: MnaState<CAPS>,
    seeded: bool,
}

/// Time an oscillator has spent high, in periods, from phase 0 to `x`.
#[inline]
fn high_time(x: f32) -> f32 {
    x.floor() * DUTY + x.fract().min(DUTY)
}

impl MetalBank {
    /// Starts the oscillators at random phases, as the 808's run freely from
    /// power-on, unless they already are. The bank rides patch updates with
    /// its channel, so a carried-over channel keeps its phases and a channel
    /// added by an update gets its own.
    pub fn seed(&mut self, base: u64, stream: usize) {
        if self.seeded {
            return;
        }
        self.seeded = true;
        let mut rng = LcgRng::default();
        rng.seed(base, stream);
        for phase in self.phases.iter_mut() {
            *phase = rng.next_unit();
        }
    }

    /// Advances one sample, returning the upper and lower bands. Each
    /// oscillator drives the sum with its mean voltage over the sample,
    /// which keeps its edges' timing between samples.
    #[inline]
    pub fn step(&mut self, filters: &MetalFilters) -> (f32, f32) {
        let mut current = 0.0;
        for (phase, &inc) in self.phases.iter_mut().zip(filters.inc.iter()) {
            let high = (high_time(*phase + inc) - high_time(*phase)) / inc;
            current += OSC_VOLTS * high / R_MIX;
            *phase = (*phase + inc).fract();
        }
        let mut injected = [0.0; UNKNOWNS];
        injected[SUM] = current;
        let x = filters.net.step(&mut self.net, 0, &injected);
        (x[BAND_HIGH], x[BAND_LOW])
    }
}
