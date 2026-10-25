//! Band-limited conversion between the engine rate and the rate the hardware
//! runs each quality at, so the chain's delay lengths, filters and aliasing
//! behave as on the module.

use std::sync::{Arc, Mutex};

/// Kernel half-width in zero crossings.
const HALF_WIDTH: f32 = 16.0;
const KAISER_BETA: f64 = 8.0;
/// Passband edge as a fraction of the lower rate's Nyquist frequency.
const CUTOFF: f32 = 0.9;
/// Fractional positions tabulated per sample; reads interpolate between them.
const PHASES: usize = 256;
/// Taps are padded to a multiple of this so the dot products vectorize.
const LANES: usize = 8;
const RING_SIZE: usize = 2048;
const RING_MASK: usize = RING_SIZE - 1;
/// Rates the qualities run at.
pub const INNER_RATES: [f32; 3] = [24000.0, 32000.0, 48000.0];

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..64 {
        term *= half / k as f64;
        sum += term * term;
        if term * term < 1e-12 * sum {
            break;
        }
    }
    sum
}

/// Kaiser-windowed sinc at `u` zero crossings.
fn kernel(u: f64) -> f64 {
    let u = u.abs();
    let half_width = HALF_WIDTH as f64;
    if u >= half_width {
        return 0.0;
    }
    let sinc = if u < 1e-9 {
        1.0
    } else {
        (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u)
    };
    let w = u / half_width;
    sinc * bessel_i0(KAISER_BETA * (1.0 - w * w).sqrt()) / bessel_i0(KAISER_BETA)
}

/// Half-width in samples of a kernel with cutoff `rho` (relative to Nyquist).
fn half_taps(rho: f32) -> usize {
    (HALF_WIDTH / rho).ceil() as usize
}

fn padded_taps(rho: f32) -> usize {
    (2 * half_taps(rho)).div_ceil(LANES) * LANES
}

/// Polyphase coefficients for one cutoff.
#[derive(Default)]
struct Table {
    /// `(PHASES + 1)` rows of `taps` coefficients.
    coefficients: Vec<f32>,
    taps: usize,
    half: usize,
}

impl Table {
    /// Build the table for cutoff `rho`. Main thread only.
    fn new(rho: f32) -> Self {
        let half = half_taps(rho);
        let taps = padded_taps(rho);
        let mut coefficients = vec![0.0; (PHASES + 1) * taps];
        for (phase, row) in coefficients.chunks_exact_mut(taps).enumerate() {
            let frac = phase as f64 / PHASES as f64;
            for (j, w) in row.iter_mut().enumerate() {
                let d = frac + half as f64 - 1.0 - j as f64;
                *w = (rho as f64 * kernel(d * rho as f64)) as f32;
            }
        }
        Self {
            coefficients,
            taps,
            half,
        }
    }
}

/// Decimation and interpolation tables for every entry of [`INNER_RATES`] at
/// one engine rate.
struct Tables {
    down: Vec<Table>,
    up: Vec<Table>,
}

/// Tables for `engine_rate`, built once per rate and shared by every
/// instance. The cache keeps a reference forever, so dropping a resampler
/// never frees them.
fn tables_for(engine_rate: f32) -> Arc<Tables> {
    static CACHE: Mutex<Vec<(u32, Arc<Tables>)>> = Mutex::new(Vec::new());
    let key = engine_rate.to_bits();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, tables)) = cache.iter().find(|(k, _)| *k == key) {
        return tables.clone();
    }
    let tables = Arc::new(Tables {
        down: INNER_RATES
            .iter()
            .map(|&inner| Table::new(CUTOFF * (inner / engine_rate).min(1.0)))
            .collect(),
        up: INNER_RATES
            .iter()
            .map(|&inner| Table::new(CUTOFF * (engine_rate / inner).min(1.0)))
            .collect(),
    });
    cache.push((key, tables.clone()));
    tables
}

/// Input history for one mono source. Every source of a [`Resampler`] is
/// written at the resampler's input index, so one read position serves all.
#[derive(Default)]
pub struct MonoHistory {
    /// Samples written twice so any window is contiguous.
    data: Vec<f32>,
}

impl MonoHistory {
    /// Allocate storage. Main thread only.
    pub fn allocate(&mut self) {
        self.data = vec![0.0; 2 * RING_SIZE];
    }

    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    /// Record the input of engine sample `index`.
    #[inline]
    pub fn write(&mut self, index: u64, x: f32) {
        let i = (index as usize) & RING_MASK;
        self.data[i] = x;
        self.data[i + RING_SIZE] = x;
    }

    #[inline]
    fn read(&self, table: &Table, position: f64) -> f32 {
        let base = position.floor();
        let frac = (position - base) as f32;
        let first = base as i64 - table.half as i64 + 1;
        if first < 0 {
            return 0.0;
        }
        let start = (first as usize) & RING_MASK;
        let taps = table.taps;

        let p = frac * PHASES as f32;
        let row = (p as usize).min(PHASES - 1);
        let t = p - row as f32;
        let row0 = &table.coefficients[row * taps..(row + 1) * taps];
        let row1 = &table.coefficients[(row + 1) * taps..(row + 2) * taps];
        let x = &self.data[start..start + taps];

        let mut acc = [0.0f32; LANES];
        for ((a, b), x) in row0
            .chunks_exact(LANES)
            .zip(row1.chunks_exact(LANES))
            .zip(x.chunks_exact(LANES))
        {
            for k in 0..LANES {
                let w = a[k] + t * (b[k] - a[k]);
                acc[k] += w * x[k];
            }
        }
        acc.iter().sum()
    }
}

/// A read position into [`MonoHistory`]s written at one resampler index.
#[derive(Clone, Copy)]
pub struct Tap<'a> {
    /// `None` when the rates match and samples pass through.
    table: Option<&'a Table>,
    position: f64,
}

impl Tap<'_> {
    /// The value of the source recorded in `history` at this position.
    #[inline]
    pub fn read(&self, history: &MonoHistory) -> f32 {
        match self.table {
            Some(table) => history.read(table, self.position),
            None => history.data[(self.position as usize) & RING_MASK],
        }
    }
}

/// One inner sample: its distance before the current engine sample and the
/// position its inputs are read at.
pub struct InnerSample<'a> {
    /// Engine samples before the current engine sample.
    pub ago: f32,
    tap: Tap<'a>,
}

impl InnerSample<'_> {
    /// This inner sample's value of the source recorded in `history`.
    #[inline]
    pub fn read(&self, history: &MonoHistory) -> f32 {
        self.tap.read(history)
    }
}

/// Runs a per-sample process at a second sample rate between inputs and
/// outputs at the engine rate. Each input and output lives in a
/// [`MonoHistory`]; inputs are written at [`Resampler::input_index`] before
/// every [`Resampler::tick`].
#[derive(Default)]
pub struct Resampler {
    bypass: bool,
    /// Inner samples per engine sample.
    ratio: f64,
    /// Engine samples the inner input lags the engine input.
    input_delay: f64,
    /// Inner samples the engine output lags the inner output.
    output_delay: f64,
    /// Inner samples elapsed since the last one was produced.
    clock: f64,
    tables: Option<Arc<Tables>>,
    /// Index into [`INNER_RATES`] of the current inner rate.
    active: usize,
    /// Engine samples ticked since configuration.
    count: u64,
    /// Inner samples produced into caller-owned output histories.
    produced: u64,
}

impl Resampler {
    /// Fetch the tables for `engine_rate`. Main thread only.
    pub fn allocate(&mut self, engine_rate: f32) {
        self.tables = Some(tables_for(engine_rate));
    }

    /// Configure for `inner_rate` (one of [`INNER_RATES`]) inside
    /// `engine_rate`; the histories are cleared by their owners.
    /// Allocation-free.
    pub fn configure(&mut self, engine_rate: f32, inner_rate: f32) {
        self.bypass = (engine_rate - inner_rate).abs() < 0.5 || self.tables.is_none();
        self.ratio = inner_rate as f64 / engine_rate as f64;
        self.clock = 0.0;
        self.count = 0;
        self.produced = 0;
        if self.bypass {
            return;
        }
        self.active = INNER_RATES
            .iter()
            .position(|&r| r == inner_rate)
            .unwrap_or(0);
        if let Some(tables) = &self.tables {
            self.input_delay = tables.down[self.active].half as f64 + 1.0;
            self.output_delay = tables.up[self.active].half as f64 + 1.0;
        }
    }

    /// Engine samples between a trigger and the inner sample that carries
    /// the audio recorded with it.
    #[inline]
    pub fn input_delay(&self) -> f32 {
        if self.bypass {
            0.0
        } else {
            self.input_delay as f32
        }
    }

    /// Inner samples per engine sample.
    #[inline]
    pub fn ratio(&self) -> f32 {
        if self.bypass { 1.0 } else { self.ratio as f32 }
    }

    /// Whether the rates differ, so inputs go through [`MonoHistory`]s.
    #[inline]
    pub fn converts(&self) -> bool {
        !self.bypass
    }

    /// Index the inputs of the current engine sample are written at.
    #[inline]
    pub fn input_index(&self) -> u64 {
        self.count
    }

    /// Advance one engine sample for outputs held in caller-owned
    /// [`MonoHistory`]s. `inner` runs once per inner sample with the index
    /// its outputs are written at; the returned [`Tap`] reads this engine
    /// sample's outputs from those histories.
    pub fn tick(&mut self, mut inner: impl FnMut(&InnerSample, u64)) -> Tap<'_> {
        let n = self.count;
        self.count += 1;
        let tables = match &self.tables {
            Some(tables) if !self.bypass => tables,
            _ => {
                let tap = Tap {
                    table: None,
                    position: n as f64,
                };
                inner(&InnerSample { ago: 0.0, tap }, n);
                return tap;
            }
        };
        let down_table = &tables.down[self.active];
        let up_table = &tables.up[self.active];

        self.clock += self.ratio;
        while self.clock >= 1.0 {
            self.clock -= 1.0;
            let ago = self.clock / self.ratio;
            let sample = InnerSample {
                ago: ago as f32,
                tap: Tap {
                    table: Some(down_table),
                    position: n as f64 - ago - self.input_delay,
                },
            };
            inner(&sample, self.produced);
            self.produced += 1;
        }

        // Before the first inner sample the position is negative and reads
        // silence.
        Tap {
            table: Some(up_table),
            position: self.produced as f64 - 1.0 + self.clock - self.output_delay,
        }
    }
}
