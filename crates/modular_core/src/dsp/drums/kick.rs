use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{AttackDecay, SILENCE, Strike, sine};
use crate::dsp::utils::{one_pole::CachedCoeff, voct_to_hz, wrap_phase};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// F1 (≈43.7 Hz) in V/Oct.
const DEFAULT_FREQ: f32 = -31.0 / 12.0;
const DEFAULT_DECAY: f32 = 1.2;
const DEFAULT_SWEEP: f32 = 0.024;

/// The sweep starts this many multiples of `freq` above it.
const SWEEP_RATIO: f32 = 3.76;
/// A phase-reset hit opens with a zap this many Hz above the body. Its time constant
/// grows from `ZAP_TAU_MIN` with no sweep to `ZAP_TAU_MAX` once `sweep`
/// reaches `ZAP_FULL_SWEEP`.
const ZAP_HZ: f32 = 684.0;
const ZAP_TAU_MIN: f32 = 0.000_85;
const ZAP_TAU_MAX: f32 = 0.002_19;
const ZAP_FULL_SWEEP: f32 = 0.0172;
/// Shortest attack, so a hit never starts with a step.
const MIN_ATTACK: f32 = 0.0005;
/// The overtone partial sits this many Hz above the body, with sidebands one
/// body frequency either side.
const OVERTONE_OFFSET_HZ: f32 = 220.0;
/// Overtone level relative to the body at full `overtone`.
const OVERTONE_LEVEL: f32 = 0.0816;
/// The overtone decays faster than the body by `level^(OVERTONE_FADE / amount)`.
const OVERTONE_FADE: f32 = 0.19;
/// Drive exponent at full `drive`. At 0 V the exponent is 1: no curve, but
/// the output still clips at full scale.
const DRIVE_MAX_EXPONENT: f32 = 10.02;
/// The click transient: a 1.33 ms rise to `CLICK_PEAK`, a short sag, a cut at
/// 1.75 ms, then a low bump that ends by 5.1 ms.
const CLICK_PEAK: f32 = 0.171;
const CLICK_RISE: f32 = 0.001_33;
const CLICK_CUT: f32 = 0.001_75;
const CLICK_SAG: f32 = 0.076;
const CLICK_TAIL: f32 = 0.021;
const CLICK_END: f32 = 0.0051;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct KickParams {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// resting pitch in V/Oct (default F1, ≈43.7 Hz)
    #[signal(type = pitch, default = -2.583, range = (-3.2, -0.4))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the hit to fall to −60 dB (default 1.2)
    #[signal(default = 1.2, range = (0.0, 10.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// time constant in seconds of the fall from the sweep's starting pitch,
    /// 4.76× `freq`, back to `freq`; 0 disables the sweep (default 0.024)
    #[signal(default = 0.024, range = (0.0, 0.1))]
    #[deserr(default)]
    sweep: Option<PolySignal>,
    /// seconds to swell to full level; softens the start of the hit (default 0)
    #[signal(default = 0.0, range = (0.0, 0.4))]
    #[deserr(default)]
    attack: Option<PolySignal>,
    /// soft-clipping amount, 0–5 V; thickens the body and lengthens the tail (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    drive: Option<PolySignal>,
    /// level of an inharmonic partial 220 Hz above the body, 0–5 V (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    overtone: Option<PolySignal>,
    /// level of a short transient on the attack, 0–5 V (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    click: Option<PolySignal>,
    /// hit strength, 0–5 V, sampled on each trigger and held for the hit; it
    /// scales the output level only (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// restart the oscillator at zero phase on every hit, with a sharp upward
    /// zap, so each hit is identical; otherwise hits land wherever the
    /// free-running body is (default false)
    #[serde(default)]
    #[deserr(default)]
    phase_reset: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct KickOutputs {
    #[output("output", "drum output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    strike: Strike,
    env: AttackDecay,
    /// Body phase in cycles; runs freely between hits unless `phase_reset`.
    phase: f32,
    overtone_phase: f32,
    /// Decaying sweep and zap amounts, 1 at the strike.
    sweep: f32,
    zap: f32,
    /// Seconds since the strike, for the click.
    elapsed: f32,
    sweep_coeff: CachedCoeff,
    zap_coeff: CachedCoeff,
}

#[derive(Default)]
struct KickState {
    overtone_inc: f32,
    sample_period: f32,
}

/// Analog-style kick drum: a sine body with a pitch sweep, struck by `trigger`.
///
/// Each rising edge on **trigger** restarts the envelope and sweeps the pitch
/// down from 4.76× **freq** with time constant **sweep**. The level then falls
/// exponentially to −60 dB over **decay** seconds. **attack** rounds off the
/// start, **drive** soft-clips the body, **overtone** adds an inharmonic
/// partial for a metallic edge, and **click** layers a short transient on top.
/// **accent**, sampled at each strike, sets the hit's level.
///
/// The body oscillator runs freely between hits, so each hit starts at a
/// different point in its cycle. **phaseReset** restarts it on every hit
/// instead, adding a sharp pitch zap and making every hit identical.
///
/// Output range is **±5 V**.
///
/// ```js
/// $kick($clock.beatTrigger, { decay: 0.6, click: 5 }).out()
///
/// // accented hits: full level on beat 1, softer elsewhere
/// $kick($clock.beatTrigger, { accent: $cycle($p('5 2.5 2.5 2.5')) }).out()
/// ```
#[module(name = "$kick", has_init, args(trigger))]
pub struct Kick {
    outputs: KickOutputs,
    params: KickParams,
    state: KickState,
    channel_state: Box<[ChannelState]>,
}

/// Click transient at `t` seconds after the strike, peaking at `CLICK_PEAK`.
fn click_shape(t: f32) -> f32 {
    if t < CLICK_RISE {
        CLICK_PEAK * (t / CLICK_RISE).powf(1.2)
    } else if t < CLICK_CUT {
        CLICK_PEAK * (1.0 - CLICK_SAG * (t - CLICK_RISE) / (CLICK_CUT - CLICK_RISE))
    } else if t < CLICK_END {
        let u = (t - CLICK_CUT) / (CLICK_END - CLICK_CUT);
        CLICK_TAIL * sine(0.5 * u)
    } else {
        0.0
    }
}

/// Odd-symmetric soft clipper that maps ±1 to ±1 and clips beyond; `exponent` 1 is linear inside ±1.
#[inline]
fn drive_curve(x: f32, exponent: f32) -> f32 {
    let mag = 1.0 - (1.0 - x.abs().min(1.0)).powf(exponent);
    mag.copysign(x)
}

impl Kick {
    fn init(&mut self, sample_rate: f32) {
        self.state.overtone_inc = OVERTONE_OFFSET_HZ / sample_rate;
        self.state.sample_period = 1.0 / sample_rate;
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
                cs.sweep = 1.0;
                cs.elapsed = 0.0;
                if p.phase_reset {
                    cs.phase = 0.0;
                    cs.overtone_phase = 0.0;
                    cs.zap = 1.0;
                }
            }

            if cs.env.active() {
                let sweep_secs = p.sweep.value_or(ch, DEFAULT_SWEEP);
                cs.sweep *= 1.0 - cs.sweep_coeff.get(sweep_secs, sample_rate);
                let zap_secs = ZAP_TAU_MIN
                    + (ZAP_TAU_MAX - ZAP_TAU_MIN) * (sweep_secs / ZAP_FULL_SWEEP).clamp(0.0, 1.0);
                cs.zap *= 1.0 - cs.zap_coeff.get(zap_secs, sample_rate);
                if cs.sweep < SILENCE {
                    cs.sweep = 0.0;
                }
                if cs.zap < SILENCE {
                    cs.zap = 0.0;
                }
            }

            let base = voct_to_hz(p.freq.value_or(ch, DEFAULT_FREQ));
            let hz = base * (1.0 + SWEEP_RATIO * cs.sweep) + ZAP_HZ * cs.zap;
            cs.phase = wrap_phase(cs.phase + hz / sample_rate);
            cs.overtone_phase = wrap_phase(cs.overtone_phase + state.overtone_inc);

            if !cs.env.active() {
                self.outputs.sample.set(ch, 0.0);
                continue;
            }

            let level = cs.env.next(
                p.attack.value_or(ch, 0.0).max(MIN_ATTACK),
                p.decay.value_or(ch, DEFAULT_DECAY),
                sample_rate,
            );
            let mut x = level * sine(cs.phase);

            let amount = p.overtone.value_or(ch, 0.0).clamp(0.0, 5.0) / 5.0;
            if amount > 0.0 {
                let partial = OVERTONE_LEVEL * amount * level.powf(1.0 + OVERTONE_FADE / amount);
                let carrier = sine(wrap_phase(cs.phase + cs.overtone_phase));
                let body_cos = sine(wrap_phase(cs.phase + 0.25));
                x += partial * carrier * (1.0 + body_cos);
            }

            let drive = p.drive.value_or(ch, 0.0).clamp(0.0, 5.0) / 5.0;
            x = drive_curve(x, 1.0 + (DRIVE_MAX_EXPONENT - 1.0) * drive);

            let click = p.click.value_or(ch, 0.0).clamp(0.0, 5.0) / 5.0;
            if click > 0.0 && cs.elapsed < CLICK_END {
                // The click lands after the drive, so it is clamped to full scale here.
                x = (x + click * click_shape(cs.elapsed)).clamp(-1.0, 1.0);
            }
            cs.elapsed += state.sample_period;

            self.outputs.sample.set(ch, x * cs.strike.velocity * 5.0);
        }
    }
}

message_handlers!(impl Kick {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48_000.0;

    fn volts(v: f32) -> Option<PolySignal> {
        Some(PolySignal::mono(Signal::Volts(v)))
    }

    fn make_kick(edit: impl FnOnce(&mut KickParams)) -> Kick {
        let mut params = KickParams {
            trigger: PolySignal::mono(Signal::Volts(0.0)),
            freq: None,
            decay: None,
            sweep: None,
            attack: None,
            drive: None,
            overtone: None,
            click: None,
            accent: None,
            phase_reset: false,
        };
        edit(&mut params);
        let mut outputs = KickOutputs::default();
        outputs.set_all_channels(1);
        let mut kick = Kick {
            params,
            outputs,
            state: KickState::default(),
            _channel_count: 1,
            _block_index: Default::default(),
            channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
        };
        kick.init(SR);
        kick
    }

    /// Strike once (a 1 ms trigger pulse) and render `secs` of output.
    fn strike(kick: &mut Kick, secs: f32) -> Vec<f32> {
        let n = (secs * SR) as usize;
        (0..n)
            .map(|i| {
                let gate = if i < 48 { 5.0 } else { 0.0 };
                kick.params.trigger = PolySignal::mono(Signal::Volts(gate));
                kick.update(SR);
                kick.outputs.sample.get(0)
            })
            .collect()
    }

    /// Render `secs` more of the current hit with the trigger held low.
    fn strike_without_trigger(kick: &mut Kick, secs: f32) -> Vec<f32> {
        kick.params.trigger = PolySignal::mono(Signal::Volts(0.0));
        (0..(secs * SR) as usize)
            .map(|_| {
                kick.update(SR);
                kick.outputs.sample.get(0)
            })
            .collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    /// Mean frequency over `samples` from its upward zero crossings.
    fn crossing_hz(samples: &[f32]) -> f32 {
        let ups: Vec<usize> = (1..samples.len())
            .filter(|&i| samples[i - 1] <= 0.0 && samples[i] > 0.0)
            .collect();
        (ups.len() - 1) as f32 * SR / (ups[ups.len() - 1] - ups[0]) as f32
    }

    #[test]
    fn silent_until_struck() {
        let mut kick = make_kick(|_| {});
        for _ in 0..4800 {
            kick.update(SR);
            assert_eq!(kick.outputs.sample.get(0), 0.0);
        }
    }

    #[test]
    fn strike_reaches_full_scale_and_decays_to_minus_60_db() {
        let mut kick = make_kick(|p| p.decay = volts(0.5));
        let out = strike(&mut kick, 0.6);
        let full = peak(&out[..4800]);
        assert!((full - 5.0).abs() < 0.05, "peak {full}");
        // One cycle of the 43.7 Hz body around the decay time.
        let at_decay = peak(&out[23_500..24_600]);
        let db = 20.0 * (at_decay / full).log10();
        assert!((db + 60.0).abs() < 1.5, "level at decay time {db} dB");
    }

    #[test]
    fn sweep_falls_back_to_freq() {
        let mut kick = make_kick(|p| {
            p.freq = volts(-1.0);
            p.sweep = volts(0.01);
        });
        let out = strike(&mut kick, 0.5);
        let early = crossing_hz(&out[..960]);
        let settled = crossing_hz(&out[9600..24_000]);
        assert!(early > 300.0, "early pitch {early} Hz");
        assert!(
            (settled - voct_to_hz(-1.0)).abs() < 0.5,
            "settled pitch {settled} Hz"
        );
    }

    #[test]
    fn no_sweep_holds_freq_from_the_start() {
        let mut kick = make_kick(|p| {
            p.freq = volts(-1.0);
            p.sweep = volts(0.0);
        });
        let out = strike(&mut kick, 0.2);
        let hz = crossing_hz(&out[..4800]);
        assert!((hz - voct_to_hz(-1.0)).abs() < 1.0, "pitch {hz} Hz");
    }

    #[test]
    fn accent_scales_level_and_holds_for_the_hit() {
        let mut full = make_kick(|_| {});
        let mut half = make_kick(|p| p.accent = volts(2.5));
        let full_out = strike(&mut full, 0.1);
        let half_out = strike(&mut half, 0.1);
        let ratio = peak(&half_out) / peak(&full_out);
        assert!((ratio - 0.5).abs() < 0.01, "ratio {ratio}");
        // Changing accent mid-hit leaves the ringing hit untouched.
        half.params.accent = volts(5.0);
        full.params.accent = volts(5.0);
        let held = strike_without_trigger(&mut half, 0.01);
        let reference = strike_without_trigger(&mut full, 0.01);
        let ratio = peak(&held) / peak(&reference);
        assert!((ratio - 0.5).abs() < 0.01, "held ratio {ratio}");
    }

    #[test]
    fn attack_delays_the_peak() {
        let mut kick = make_kick(|p| {
            p.attack = volts(0.1);
            p.sweep = volts(0.0);
        });
        let out = strike(&mut kick, 0.3);
        let at = out
            .iter()
            .enumerate()
            .fold(
                (0, 0.0),
                |(bi, bv), (i, s)| if s.abs() > bv { (i, s.abs()) } else { (bi, bv) },
            )
            .0;
        let ms = at as f32 / SR * 1000.0;
        assert!((80.0..=115.0).contains(&ms), "peak at {ms} ms");
    }

    #[test]
    fn phase_reset_makes_every_hit_from_silence_identical() {
        let mut kick = make_kick(|p| {
            p.phase_reset = true;
            p.decay = volts(0.05);
        });
        let first = strike(&mut kick, 0.25);
        let second = strike(&mut kick, 0.25);
        assert_eq!(first, second);
    }

    #[test]
    fn free_running_retrigger_swells_without_a_step() {
        let mut kick = make_kick(|p| p.sweep = volts(0.0));
        strike(&mut kick, 0.05);
        let before = kick.outputs.sample.get(0);
        let after = strike(&mut kick, 0.001)[0];
        // The shortest attack spans 24 samples, so a retrigger rises by at most
        // full scale / 24 per sample.
        assert!(
            (after - before).abs() < 5.0 / 24.0,
            "step {before} -> {after}"
        );
    }

    #[test]
    fn drive_overtone_and_click_stay_within_full_scale() {
        let mut kick = make_kick(|p| {
            p.drive = volts(5.0);
            p.overtone = volts(5.0);
            p.click = volts(5.0);
            p.phase_reset = true;
        });
        let out = strike(&mut kick, 0.3);
        assert!(peak(&out) <= 5.0 + 1e-4, "peak {}", peak(&out));
    }

    #[test]
    fn click_adds_a_transient_only_at_the_strike() {
        let mut plain = make_kick(|p| p.phase_reset = true);
        let mut clicked = make_kick(|p| {
            p.phase_reset = true;
            p.click = volts(5.0);
        });
        let a = strike(&mut plain, 0.05);
        let b = strike(&mut clicked, 0.05);
        let diff: Vec<f32> = a.iter().zip(&b).map(|(x, y)| y - x).collect();
        assert!(
            peak(&diff[..250]) > 0.8,
            "click peak {}",
            peak(&diff[..250])
        );
        assert_eq!(peak(&diff[250..]), 0.0);
    }
}
