use super::*;

/// Shortest and longest `decay` the feedback can reach, seconds.
const MIN_DECAY: f32 = 0.01;
const MAX_DECAY: f32 = 10.0;

/// The bridged-T tuned for one `freq` and `decay`, and the output gain
/// levelling `freq`.
#[derive(Clone, Copy)]
pub(super) struct Network {
    pub(super) freq: f32,
    pub(super) decay: f32,
    pub(super) t: BridgedT,
    pub(super) level: f32,
}

impl Default for Network {
    fn default() -> Self {
        // NaN never compares equal, so the first sample always computes.
        Self {
            freq: f32::NAN,
            decay: f32::NAN,
            t: BridgedT {
                c_a: 1.0,
                c_b: 1.0,
                r_bridge: R228,
                g_fb: 0.0,
            },
            level: 1.0,
        }
    }
}

impl Network {
    pub(super) fn new(parts: &Parts, freq: f32, decay: f32, rest_hz: f32) -> Self {
        let r = voct_to_hz(freq) / rest_hz;
        Self {
            freq,
            decay,
            t: stock_t(parts).tuned(r, decay.clamp(MIN_DECAY, MAX_DECAY)),
            level: parts.level.gain(freq),
        }
    }
}

/// A voice's bridged-T as built.
fn stock_t(parts: &Parts) -> BridgedT {
    BridgedT {
        c_a: parts.c_a,
        c_b: parts.c_b,
        r_bridge: R228,
        g_fb: 1.0 / R230,
    }
}

/// Resting frequency of a voice's bridged-T, Hz.
pub(super) fn rest_hz(parts: &Parts) -> f32 {
    stock_t(parts).rest_hz(1.0 / (R231 + POT_B))
}

/// Largest junction voltage the tuning table covers, and its resolution.
const TUNE_MAX: f32 = 16.0;
const TUNE_STEPS: usize = 2048;

/// Current through the tuning network for junction voltages 0–[`TUNE_MAX`].
/// The network is odd-symmetric, so negative voltages read it mirrored.
static TUNING: OnceLock<Box<[f32]>> = OnceLock::new();

pub(super) fn tuning_table() -> &'static [f32] {
    TUNING.get_or_init(|| {
        (0..=TUNE_STEPS)
            .map(|j| tuning_current_exact(TUNE_MAX * j as f32 / TUNE_STEPS as f32))
            .collect()
    })
}

/// Current through R231, the pot and D80/D81 from a junction voltage `x`.
/// Newton in the diode current rather than its voltage converges from any
/// start, since the voltage is only logarithmic in the current.
fn tuning_current_exact(x: f32) -> f32 {
    let (x, k) = (x as f64, 1.0 + (POT_B / R231) as f64);
    let (is, nvt, pa, pb) = (GE_IS as f64, GE_NVT as f64, POT_A as f64, POT_B as f64);
    let (mut i, mut vd) = (0.0f64, 0.0f64);
    for _ in 0..100 {
        vd = nvt * (i / (2.0 * is)).asinh();
        let dvd = nvt / (i * i + 4.0 * is * is).sqrt();
        let f = (vd + i * pa) * k + pb * i - x;
        i -= f / ((dvd + pa) * k + pb);
    }
    ((x - vd - i * pa) / pb) as f32
}

/// The tuning network's current at `x` and its slope, interpolated, lane by
/// lane.
#[inline]
pub(super) fn tuning_current<L: Lanes>(table: &[f32], x: L::S) -> (L::S, L::S) {
    let (mut current, mut slope) = (c::<L::S>(0.0), c::<L::S>(0.0));
    for lane in 0..L::WIDTH {
        let (i, g) = tuning_current_at(table, x.lane(lane));
        current.set_lane(lane, &i);
        slope.set_lane(lane, &g);
    }
    (current, slope)
}

/// The tuning network's current at `x` and its slope, interpolated.
#[inline]
fn tuning_current_at(table: &[f32], x: f32) -> (f32, f32) {
    let pos = x.abs().min(TUNE_MAX) * (TUNE_STEPS as f32 / TUNE_MAX);
    let j = (pos as usize).min(TUNE_STEPS - 1);
    let (lo, hi) = (table[j], table[j + 1]);
    let current = lo + (pos - j as f32) * (hi - lo);
    (
        current.copysign(x),
        (hi - lo) * (TUNE_STEPS as f32 / TUNE_MAX),
    )
}
