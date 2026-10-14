use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{Decay, Strike, Svf, sine};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{one_pole::OnePole, voct_to_hz, wrap_phase};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// ≈E4 (322 Hz) in V/Oct.
const DEFAULT_FREQ: f32 = 0.3;
const DEFAULT_DECAY: f32 = 0.33;
const DEFAULT_TONE: f32 = 3.7;
const DEFAULT_REPEAT: f32 = 1.8;

/// The upper square sits this far above `freq`, giving the cowbell's
/// inharmonic interval. The lower voice pairs a square at `freq` with a
/// louder one `OVERTONE_RATIO` above it.
const UPPER_RATIO: f32 = 1.535;
const OVERTONE_RATIO: f32 = 3.025;
const UPPER_LEVEL: f32 = 0.36;
const LOWER_LEVEL: f32 = 0.21;
const OVERTONE_LEVEL: f32 = 0.39;
/// The bell rings on an exponential that would reach −60 dB at
/// `RING_STRETCH` times `decay`, but is cut off at `decay` with a short fade.
const RING_STRETCH: f32 = 1.6;
const CUT_FADE_SECS: f32 = 0.02;
/// The noise rises as `noise^NOISE_CURVE`, gently high-passed at
/// `NOISE_HP_HZ`, and fades to −60 dB over `NOISE_DECAY` of `decay`.
const NOISE_LEVEL: f32 = 0.24;
const NOISE_CURVE: f32 = 0.83;
const NOISE_HP_HZ: f32 = 600.0;
const NOISE_DECAY: f32 = 0.6;
/// `filter` f (0–1) sweeps a high-pass up towards `FILTER_MAX_HZ`, at
/// `FILTER_MAX_HZ · (1 − e^(−f / FILTER_SWEEP))`, lifted by
/// `FILTER_GAIN · (1 + FILTER_BOOST · f)`, over a dry signal that falls from
/// `FILTER_FLOOR + FILTER_DRY` at f = 0.1 towards `FILTER_FLOOR`.
const FILTER_MAX_HZ: f32 = 13_000.0;
const FILTER_SWEEP: f32 = 0.45;
const FILTER_Q: f32 = 0.6;
const FILTER_GAIN: f32 = 2.0;
const FILTER_BOOST: f32 = 1.8;
const FILTER_FLOOR: f32 = 0.1;
const FILTER_DRY: f32 = 0.4;
const FILTER_DRY_FALL: f32 = 0.15;
/// The clave: a tone at `CLAVE_HZ` falling 60 dB over `CLAVE_DECAY` seconds,
/// struck again `FLAM_SECS` later at `FLAM_LEVEL`. With any `repeat` r
/// (0–1) it keeps re-striking every `REPEAT_MAX_SECS`, or every
/// `REPEAT_BASE_SECS − REPEAT_SPAN_SECS · r` once that is shorter, starting
/// at `REPEAT_LEVEL` and fading `REPEAT_FADE_DB` per second until the bell
/// stops ringing.
const CLAVE_HZ: f32 = 3130.0;
const CLAVE_DECAY: f32 = 0.19;
const CLAVE_LEVEL: f32 = 0.8;
const FLAM_SECS: f32 = 0.011;
const FLAM_LEVEL: f32 = 0.8;
const REPEAT_MAX_SECS: f32 = 0.1;
const REPEAT_BASE_SECS: f32 = 0.1488;
const REPEAT_SPAN_SECS: f32 = 0.1366;
const REPEAT_LEVEL: f32 = 0.16;
const REPEAT_FADE_DB: f32 = 33.0;
/// Level of the softest accent.
const ACCENT_FLOOR: f32 = 0.08;
/// Output scale that puts a default hit's peak near full scale.
const OUTPUT_GAIN: f32 = 0.75;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ClangParams {
    /// rising edges strike the bell
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// pitch of the lower square in V/Oct; the upper one sits 1.535× above it
    /// (default ≈E4)
    #[signal(type = pitch, default = 0.3, range = (-1.3, 2.1))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the hit to fall to −60 dB (default 0.33)
    #[signal(default = 0.33, range = (0.0, 3.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// level of the upper square, 0–5 V (default 3.7)
    #[signal(default = 3.7, range = (0.0, 5.0))]
    #[deserr(default)]
    high: Option<PolySignal>,
    /// level of the lower square, 0–5 V (default 3.7)
    #[signal(default = 3.7, range = (0.0, 5.0))]
    #[deserr(default)]
    low: Option<PolySignal>,
    /// resonant high-pass sweep, 0–5 V: thins the bell and brightens its top;
    /// 0 leaves it unfiltered (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    filter: Option<PolySignal>,
    /// level of added noise, 0–5 V (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    noise: Option<PolySignal>,
    /// layer a clave click on top (default false)
    #[serde(default)]
    #[deserr(default)]
    clave: bool,
    /// clave re-strikes, 0–5 V: off at 0, then every 100 ms, quickening to
    /// every 12 ms at full (default 1.8)
    #[signal(default = 1.8, range = (0.0, 5.0))]
    #[deserr(default)]
    repeat: Option<PolySignal>,
    /// hit strength, 0–5 V, sampled on each trigger and held for the hit; it
    /// scales the output level only (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClangOutputs {
    #[output("output", "bell output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    strike: Strike,
    env: Decay,
    noise_env: Decay,
    clave_env: Decay,
    lower_phase: f32,
    overtone_phase: f32,
    upper_phase: f32,
    /// Seconds since the strike, for the cut-off at `decay`.
    elapsed: f32,
    clave_phase: f32,
    /// Seconds until the clave's next re-strike.
    repeat_wait: f32,
    flammed: bool,
    rng: LcgRng,
    noise_low: OnePole,
    sweep: Svf,
}

#[derive(Default)]
struct ClangState {
    sample_period: f32,
    /// One-pole coefficient of the noise high-pass; depends only on the sample rate.
    noise_coeff: f32,
    seeded: bool,
}

/// Seconds between clave re-strikes for `repeat` in volts, or `None` when off.
#[inline]
fn repeat_period(repeat: f32) -> Option<f32> {
    let r = repeat.clamp(0.0, 5.0) / 5.0;
    (r > 0.0).then(|| REPEAT_MAX_SECS.min(REPEAT_BASE_SECS - REPEAT_SPAN_SECS * r))
}

/// Square wave with PolyBLEP-smoothed edges, in ±1.
#[inline]
fn square(phase: f32, dt: f32) -> f32 {
    let naive = if phase < 0.5 { 1.0 } else { -1.0 };
    naive + blep(phase, dt) - blep(wrap_phase(phase + 0.5), dt)
}

/// PolyBLEP residual for a unit rising step at phase 0.
#[inline]
fn blep(phase: f32, dt: f32) -> f32 {
    if phase < dt {
        let t = phase / dt;
        2.0 * t - t * t - 1.0
    } else if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

/// Clang: a metallic cowbell from two squares an inharmonic interval apart,
/// with an optional clave.
///
/// Each rising edge on **trigger** strikes two squares, the lower at **freq**
/// and the upper 1.535× above it, mixed by **low** and **high**. **filter**
/// sweeps a resonant high-pass over them and **noise** adds hiss; everything
/// falls to −60 dB over **decay** seconds. **clave** layers a bright click
/// that re-strikes in quick succession as **repeat** rises. **accent**,
/// sampled at each strike, sets the hit's level.
///
/// Output range is **±5 V**.
///
/// ```js
/// $clang($clock.beatTrigger, { freq: 'g4', clave: true }).out()
/// ```
#[module(name = "$clang", has_init, patch_update, args(trigger))]
pub struct Clang {
    outputs: ClangOutputs,
    params: ClangParams,
    state: ClangState,
    channel_state: Box<[ChannelState]>,
}

impl Clang {
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_period = 1.0 / sample_rate;
        self.state.noise_coeff = 1.0 - (-std::f32::consts::TAU * NOISE_HP_HZ / sample_rate).exp();
    }

    fn update(&mut self, sample_rate: f32) {
        let state = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];

            if cs
                .strike
                .process(p.trigger.get_value(ch), p.accent.value_or(ch, 5.0))
            {
                cs.env.strike();
                cs.noise_env.strike();
                cs.elapsed = 0.0;
                if p.clave {
                    cs.clave_env.strike();
                    cs.clave_phase = 0.0;
                    cs.flammed = false;
                    cs.repeat_wait = repeat_period(p.repeat.value_or(ch, DEFAULT_REPEAT))
                        .unwrap_or(f32::INFINITY);
                }
            }
            if !cs.env.active() && !cs.clave_env.active() {
                self.outputs.sample.set(ch, 0.0);
                continue;
            }

            let decay = p.decay.value_or(ch, DEFAULT_DECAY);
            let mut cut = 1.0;
            let past_cut = cs.elapsed - decay;
            if past_cut > 0.0 {
                cut = (1.0 - past_cut / CUT_FADE_SECS).max(0.0);
                if past_cut >= CUT_FADE_SECS {
                    cs.env.level = 0.0;
                    cs.noise_env.level = 0.0;
                }
            }
            let level = cut * cs.env.next(decay * RING_STRETCH, sample_rate);
            let noise_level = cut * cs.noise_env.next(decay * NOISE_DECAY, sample_rate);
            cs.elapsed += state.sample_period;
            let hz = voct_to_hz(p.freq.value_or(ch, DEFAULT_FREQ));
            let lower_dt = (hz / sample_rate).min(0.5);
            let overtone_dt = (hz * OVERTONE_RATIO / sample_rate).min(0.5);
            let upper_dt = (hz * UPPER_RATIO / sample_rate).min(0.5);
            cs.lower_phase = wrap_phase(cs.lower_phase + lower_dt);
            cs.overtone_phase = wrap_phase(cs.overtone_phase + overtone_dt);
            cs.upper_phase = wrap_phase(cs.upper_phase + upper_dt);

            let low = p.low.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let high = p.high.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let lower = LOWER_LEVEL * square(cs.lower_phase, lower_dt)
                + OVERTONE_LEVEL * square(cs.overtone_phase, overtone_dt);
            let upper = UPPER_LEVEL * square(cs.upper_phase, upper_dt);
            let noise = (p.noise.value_or(ch, 0.0).clamp(0.0, 5.0) / 5.0).powf(NOISE_CURVE);
            let white = cs.rng.next_bipolar();
            cs.noise_low.set_coeff(state.noise_coeff);
            let hiss = white - cs.noise_low.process(white);
            let mut x =
                level * (low * lower + high * upper) + noise_level * NOISE_LEVEL * noise * hiss;

            let sweep = p.filter.value_or(ch, 0.0).clamp(0.0, 5.0) / 5.0;
            if sweep > 0.0 {
                let cutoff = FILTER_MAX_HZ * (1.0 - (-sweep / FILTER_SWEEP).exp());
                let high = cs.sweep.process(x, cutoff, FILTER_Q, sample_rate).hp;
                let dry = FILTER_FLOOR + FILTER_DRY * (-(sweep - 0.1) / FILTER_DRY_FALL).exp();
                x = dry * x + FILTER_GAIN * (1.0 + FILTER_BOOST * sweep) * high;
            }

            if cs.clave_env.active() {
                if !cs.flammed && cs.elapsed >= FLAM_SECS {
                    cs.clave_env.level = cs.clave_env.level.max(FLAM_LEVEL);
                    cs.flammed = true;
                }
                if cs.elapsed < RING_STRETCH * decay {
                    cs.repeat_wait -= state.sample_period;
                    if cs.repeat_wait <= 0.0 {
                        let fade = 10f32.powf(-REPEAT_FADE_DB / 20.0 * cs.elapsed);
                        cs.clave_env.level = cs.clave_env.level.max(REPEAT_LEVEL * fade);
                        cs.repeat_wait += repeat_period(p.repeat.value_or(ch, DEFAULT_REPEAT))
                            .unwrap_or(f32::INFINITY);
                    }
                }
                let clave = cs.clave_env.next(CLAVE_DECAY, sample_rate);
                cs.clave_phase = wrap_phase(cs.clave_phase + CLAVE_HZ / sample_rate);
                x += CLAVE_LEVEL * clave * sine(cs.clave_phase);
            }

            self.outputs
                .sample
                .set(ch, x * cs.strike.gain(ACCENT_FLOOR) * OUTPUT_GAIN * 5.0);
        }
    }
}

impl crate::types::PatchUpdateHandler for Clang {
    /// Seeds each channel's noise off this module's stable heap address once,
    /// so the stream carries over unchanged across patch updates.
    fn on_patch_update(&mut self) {
        if !self.state.seeded {
            let base = seed_base(self);
            for (ch, cs) in self.channel_state.iter_mut().enumerate() {
                cs.rng.seed(base, ch);
            }
            self.state.seeded = true;
        }
    }
}

message_handlers!(impl Clang {});
