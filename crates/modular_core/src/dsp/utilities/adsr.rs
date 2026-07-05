use crate::dsp::utils::SchmittTrigger;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use deserr::Deserr;
use schemars::JsonSchema;

/// Times below this (in seconds) saturate a ramp's time phase in one sample;
/// the level itself still slews at [`MAX_LEVEL_STEP`] per sample.
const TIME_EPSILON: f32 = 0.0001;

/// Duration of the fast fade-to-zero a `resetOnRetrig` retrigger runs before
/// re-attacking, so the snap back to the bottom doesn't click.
const RETRIG_RESET_TIME: f32 = 0.005;

/// Maximum magnitude of the curve coefficient, reached at `curve = ±5`.
/// ln(1000) ≈ 6.9
const CURVE_STRENGTH: f32 = 6.9;

/// The attack uses a gentler curve than the decay/release — a fully shaped
/// attack feels abrupt — so only this fraction of the curve coefficient is
/// applied to it.
const ATTACK_CURVE_SCALE: f32 = 0.5;

/// Largest change in the normalized envelope level (0..1) allowed in a single
/// sample. Any ramp steeper than this — a near-instant attack, or a steep curve
/// at a low sample rate — is slewed at this rate instead, so the onset cannot
/// click no matter the attack time or sample rate. 0.02 of the 0–5V output is
/// 0.1V, small enough to be inaudible as a step.
const MAX_LEVEL_STEP: f32 = 0.02;

/// Maps a normalized ramp position `phase` in 0..1 to a shaped value in 0..1.
/// `c` is the curve coefficient: 0 is
/// linear, `c < 0` is concave (fast then slow), `c > 0` is convex (slow then
/// fast). Unlike a power curve it has finite slope at both ends, so even steep
/// settings transition smoothly instead of clicking.
#[inline]
fn curve_shape(phase: f32, c: f32) -> f32 {
    if c.abs() < 1e-3 {
        phase
    } else {
        (1.0 - (c * phase).exp()) / (1.0 - c.exp())
    }
}

/// Inverse of [`curve_shape`]: the position whose shaped value equals `level`
/// (for `level` in 0..1). Lets a retrigger resume from the current level.
#[inline]
fn curve_shape_inverse(level: f32, c: f32) -> f32 {
    if c.abs() < 1e-3 {
        level
    } else {
        (1.0 - level * (1.0 - c.exp())).ln() / c
    }
}

/// Advance a curved ramp from `start` toward `target` by one sample, returning
/// `true` once it has arrived. `state.phase` is the linear-in-time position
/// (advanced by `dphase`); `c` is the curve coefficient.
///
/// The level is integrated as a running sum of per-sample deltas rather than
/// recomputed from `phase` each call, so a mid-ramp change in `target` (a live
/// `sustain`) or in `c` (a live `curve`) only bends the *remaining* trajectory
/// — the output never jumps. Each step is capped at [`MAX_LEVEL_STEP`], so an
/// arbitrarily short ramp slews to its target instead of clicking; when the cap
/// holds the level back, the ramp finishes only once the level has caught up.
#[inline]
fn advance_ramp(state: &mut ChannelState, start: f32, target: f32, c: f32, dphase: f32) -> bool {
    if state.phase < 1.0 {
        let phase_prev = state.phase;
        state.phase = (state.phase + dphase).min(1.0);
        let span = target - start;
        let delta = span * (curve_shape(state.phase, c) - curve_shape(phase_prev, c));
        state.current_level += delta.clamp(-MAX_LEVEL_STEP, MAX_LEVEL_STEP);
        // Finish only once the time ramp is complete *and* the level has arrived;
        // a capped step may have left it short, in which case we keep slewing
        // below.
        if state.phase >= 1.0 && (target - state.current_level).abs() <= MAX_LEVEL_STEP {
            state.current_level = target;
            return true;
        }
        return false;
    }

    // Phase saturated but the per-sample cap left the level short of the target
    // (a near-instant ramp, or a steep curve at a low sample rate). Slew the
    // remainder one capped step at a time.
    let remaining = target - state.current_level;
    if remaining.abs() <= MAX_LEVEL_STEP {
        state.current_level = target;
        return true;
    }
    state.current_level += MAX_LEVEL_STEP.copysign(remaining);
    false
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct AdsrParams {
    /// gate input — rising edge starts the envelope, falling edge triggers release
    #[signal(type = gate, range = (0.0, 5.0))]
    gate: PolySignal,
    /// attack time in seconds
    #[signal(default = 0.001, range = (0.0, 10.0))]
    #[deserr(default)]
    attack: Option<PolySignal>,
    /// decay time in seconds
    #[signal(default = 0.05, range = (0.0, 10.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// sustain level in volts (0-5). Defaults to 5; if decay is set but sustain is
    /// omitted, sustain defaults to 0 (a plucky AD shape).
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    sustain: Option<PolySignal>,
    /// release time in seconds
    #[signal(default = 0.01, range = (0.0, 10.0))]
    #[deserr(default)]
    release: Option<PolySignal>,
    /// shape of the attack, decay, and release ramps. 0 is linear; +5 is
    /// logarithmic attack with exponential decay/release; -5 is exponential
    /// attack with logarithmic decay/release.
    /// Defaults to 5.
    #[signal(default = 5.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    curve: Option<PolySignal>,
    /// retrigger input — a rising edge restarts the attack phase. The current
    /// output level is preserved: the envelope resumes along the attack curve
    /// from wherever it currently is. Defaults to the gate input when omitted.
    #[signal(type = gate, range = (0.0, 5.0))]
    #[deserr(default)]
    retrigger: Option<PolySignal>,
    /// when true, a retrigger jumps back to the very start of the attack
    /// instead of resuming from the current level.
    #[serde(default)]
    #[deserr(default)]
    reset_on_retrig: bool,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum EnvelopeStage {
    #[default]
    Idle,
    /// Brief fade to zero before a `resetOnRetrig` retrigger re-attacks.
    Retrigger,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Per-channel envelope state.
#[derive(Clone, Copy)]
struct ChannelState {
    stage: EnvelopeStage,
    /// Current envelope level in `[0, 1]`; scaled to 0–5V on output.
    current_level: f32,
    /// Level captured when the active ramp began, so a ramp interrupted partway
    /// (e.g. a re-trigger during release) continues from where it was.
    stage_start_level: f32,
    /// Position within the active ramp in `[0, 1)`, advanced linearly in time and
    /// shaped by the curve coefficient for the output level.
    phase: f32,
    gate_schmitt: SchmittTrigger,
    retrig_schmitt: SchmittTrigger,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            stage: EnvelopeStage::Idle,
            current_level: 0.0,
            stage_start_level: 0.0,
            phase: 0.0,
            gate_schmitt: SchmittTrigger::default(),
            retrig_schmitt: SchmittTrigger::default(),
        }
    }
}

/// An Attack-Decay-Sustain-Release envelope generator.
///
/// Generates a control voltage envelope driven by a **gate** input.
/// When the gate goes high (>1V) the envelope enters the attack phase;
/// when the gate goes low it enters release.
///
/// - **attack** / **decay** / **release** — time in seconds
/// - **sustain** — level in volts (0–5V). Defaults to 5V, but if **decay** is set
///   without **sustain**, sustain defaults to 0V for a plucky attack-decay shape.
/// - **curve** — ramp shape from -5 to 5. 0 is linear; +5 gives a logarithmic
///   attack and exponential decay/release; -5 is the inverse.
///   Defaults to 5.
/// - **retrigger** — a rising edge restarts the attack from the current level
///   (no jump). Defaults to the gate input. Set **resetOnRetrig** to true to
///   instead snap back to the start of the attack on every retrigger.
///
/// Output range is **0–5V**.
///
/// ## Example
///
/// ```js
/// const env = $adsr($pPulse($clock[0]), { attack: 0.01, decay: 0.2, sustain: 3, release: 0.5 })
/// $sine('c4').amplitude(env).out()
/// ```
#[module(name = "$adsr", args(gate))]
pub struct Adsr {
    outputs: AdsrOutputs,
    channel_state: Box<[ChannelState]>,
    params: AdsrParams,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct AdsrOutputs {
    #[output("output", "envelope output", default, range = (0.0, 5.0))]
    sample: PolyOutput,
}

impl Adsr {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        // When decay is set explicitly but sustain is omitted, sustain defaults to
        // 0V (a plucky attack-decay shape); otherwise it defaults to full 5V.
        let sustain_default = if self.params.sustain.is_none() && self.params.decay.is_some() {
            0.0
        } else {
            5.0
        };

        let reset_on_retrig = self.params.reset_on_retrig;

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            let attack = self.params.attack.value_or(ch, 0.001).max(0.0);
            let decay = self.params.decay.value_or(ch, 0.05).max(0.0);
            let release = self.params.release.value_or(ch, 0.01).max(0.0);
            let sustain = self
                .params
                .sustain
                .value_or(ch, sustain_default)
                .clamp(0.0, 5.0);
            let curve = self.params.curve.value_or(ch, 5.0).clamp(-5.0, 5.0);

            // Curve coefficient shared by every ramp (see `curve_shape`). A
            // negative coefficient bends a ramp concave (fast then slow), positive
            // convex (slow then fast). Because "log" and "exp" swap meaning between
            // a rising ramp (attack) and a falling one (decay/release), one
            // coefficient yields a logarithmic attack with exponential
            // decay/release at curve = +5, and the inverse at -5.
            let c = -(curve / 5.0) * CURVE_STRENGTH;
            // The attack gets a gentler curve so a fast re-attack doesn't click.
            let c_attack = c * ATTACK_CURVE_SCALE;

            let sustain_level = (sustain / 5.0).clamp(0.0, 1.0);

            let gate_val = self.params.gate.get_value(ch);
            let (gate_on, gate_edge) = state.gate_schmitt.process_with_edge(gate_val);

            // The retrigger input defaults to the gate when not patched.
            let retrig_val = self.params.retrigger.value_or(ch, gate_val);
            let (_, retrig_edge) = state.retrig_schmitt.process_with_edge(retrig_val);

            if gate_edge.is_rising() {
                state.stage = EnvelopeStage::Attack;
                state.phase = 0.0;
                state.stage_start_level = state.current_level;
            } else if gate_edge.is_falling() {
                state.stage = EnvelopeStage::Release;
                state.phase = 0.0;
                state.stage_start_level = state.current_level;
            }

            // A retrigger restarts the attack. Processed after the gate edges so it
            // takes precedence when retrigger and gate fire together.
            if retrig_edge.is_rising() {
                if reset_on_retrig {
                    // Snap back to the start of the attack, but fade the current
                    // level down to zero over a few milliseconds first so the reset
                    // doesn't click. Skip the fade when we're already near zero
                    // (e.g. a fresh note) so onsets aren't delayed.
                    if state.current_level > TIME_EPSILON {
                        state.stage = EnvelopeStage::Retrigger;
                        state.phase = 0.0;
                        state.stage_start_level = state.current_level;
                    } else {
                        state.stage = EnvelopeStage::Attack;
                        state.phase = 0.0;
                        state.stage_start_level = 0.0;
                    }
                } else {
                    // Hold the output continuous: the attack ramp starts at level 0
                    // but its phase is seeked to the point whose shaped value equals
                    // the current level, so the envelope resumes along the curve from
                    // where it is instead of jumping.
                    state.stage = EnvelopeStage::Attack;
                    state.stage_start_level = 0.0;
                    state.phase =
                        curve_shape_inverse(state.current_level.clamp(0.0, 1.0), c_attack);
                }
            }

            match state.stage {
                EnvelopeStage::Idle => {
                    state.current_level = 0.0;
                }
                EnvelopeStage::Retrigger => {
                    // Quick linear fade from the captured level to zero, then begin
                    // the attack from the bottom.
                    state.phase += 1.0 / (RETRIG_RESET_TIME * sample_rate);
                    if state.phase >= 1.0 {
                        state.current_level = 0.0;
                        state.stage = EnvelopeStage::Attack;
                        state.phase = 0.0;
                        state.stage_start_level = 0.0;
                    } else {
                        state.current_level = state.stage_start_level * (1.0 - state.phase);
                    }
                }
                EnvelopeStage::Attack => {
                    // A sub-epsilon attack saturates the time ramp in one sample;
                    // advance_ramp's step cap then slews the level to the top so
                    // even a zero attack cannot click.
                    let start = state.stage_start_level;
                    let dphase = if attack < TIME_EPSILON {
                        1.0
                    } else {
                        1.0 / (attack * sample_rate)
                    };
                    if advance_ramp(state, start, 1.0, c_attack, dphase) {
                        state.stage = EnvelopeStage::Decay;
                        state.phase = 0.0;
                        state.stage_start_level = 1.0;
                    }
                }
                EnvelopeStage::Decay => {
                    let start = state.stage_start_level;
                    let dphase = if decay < TIME_EPSILON {
                        1.0
                    } else {
                        1.0 / (decay * sample_rate)
                    };
                    if advance_ramp(state, start, sustain_level, c, dphase) {
                        state.stage = EnvelopeStage::Sustain;
                    }
                }
                EnvelopeStage::Sustain => {
                    state.current_level = sustain_level;
                    if !gate_on {
                        state.stage = EnvelopeStage::Release;
                        state.phase = 0.0;
                        state.stage_start_level = state.current_level;
                    }
                }
                EnvelopeStage::Release => {
                    let start = state.stage_start_level;
                    let dphase = if release < TIME_EPSILON {
                        1.0
                    } else {
                        1.0 / (release * sample_rate)
                    };
                    if advance_ramp(state, start, 0.0, c, dphase) {
                        state.phase = 0.0;
                        state.stage_start_level = 0.0;
                        state.stage = if gate_on {
                            EnvelopeStage::Attack
                        } else {
                            EnvelopeStage::Idle
                        };
                    }
                }
            }

            self.outputs.sample.set(ch, state.current_level * 5.0);
        }
    }
}

message_handlers!(impl Adsr {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48_000.0;

    fn make(params: AdsrParams) -> Adsr {
        let mut outputs = AdsrOutputs::default();
        outputs.set_all_channels(1);
        Adsr {
            params,
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
        }
    }

    fn gate(volts: f32) -> PolySignal {
        PolySignal::mono(Signal::Volts(volts))
    }

    fn time(secs: f32) -> Option<PolySignal> {
        Some(PolySignal::mono(Signal::Volts(secs)))
    }

    /// Run the envelope for `n` samples and return the final output (0–5V).
    fn run(adsr: &mut Adsr, n: usize) -> f32 {
        for _ in 0..n {
            adsr.update(SR);
        }
        adsr.outputs.sample.get(0)
    }

    fn base_params() -> AdsrParams {
        AdsrParams {
            gate: gate(5.0),
            attack: None,
            decay: None,
            sustain: None,
            release: None,
            curve: None,
            retrigger: None,
            reset_on_retrig: false,
        }
    }

    #[test]
    fn defaults_sustain_at_full_level() {
        // No decay set → sustain defaults to 5V. After attack + decay the level
        // should settle at the top of the range.
        let mut adsr = make(base_params());
        let out = run(&mut adsr, (SR * 0.5) as usize);
        assert!(out > 4.99, "default sustain should hold near 5V, got {out}");
    }

    #[test]
    fn decay_without_sustain_falls_to_zero() {
        // Decay set, sustain omitted → sustain defaults to 0V. The level should
        // decay to silence even while the gate is held high.
        let mut adsr = make(AdsrParams {
            decay: time(0.05),
            ..base_params()
        });
        let out = run(&mut adsr, (SR * 0.5) as usize);
        assert!(
            out < 0.01,
            "decay-without-sustain should settle at 0V, got {out}"
        );
    }

    #[test]
    fn decay_with_explicit_sustain_holds_level() {
        // Decay set and sustain set explicitly → the explicit level wins.
        let mut adsr = make(AdsrParams {
            decay: time(0.05),
            sustain: time(2.0),
            ..base_params()
        });
        let out = run(&mut adsr, (SR * 0.5) as usize);
        assert!(
            (out - 2.0).abs() < 0.05,
            "explicit sustain should hold ~2V, got {out}"
        );
    }

    #[test]
    fn gate_release_returns_to_zero() {
        let mut adsr = make(base_params());
        run(&mut adsr, (SR * 0.2) as usize);
        adsr.params.gate = gate(0.0);
        let out = run(&mut adsr, (SR * 0.5) as usize);
        assert!(
            out < 0.01,
            "envelope should return to 0V after release, got {out}"
        );
    }

    #[test]
    fn retrigger_restarts_attack_without_dropping_output() {
        // Separate retrigger input held low while the gate brings the envelope up
        // to its sustain level.
        let mut adsr = make(AdsrParams {
            attack: time(0.01),
            decay: time(0.1),
            sustain: time(2.0),
            retrigger: Some(gate(0.0)),
            ..base_params()
        });
        let sustained = run(&mut adsr, (SR * 0.4) as usize);
        assert!(
            (sustained - 2.0).abs() < 0.05,
            "should settle at sustain ~2V, got {sustained}"
        );

        // Rising edge on the retrigger input: output must not jump to 0.
        adsr.params.retrigger = Some(gate(5.0));
        adsr.update(SR);
        let after = adsr.outputs.sample.get(0);
        assert!(
            (after - sustained).abs() < 0.1,
            "retrigger should resume from the current level, got {after} (was {sustained})"
        );

        // The attack then climbs back toward the peak.
        let climbed = run(&mut adsr, (SR * 0.005) as usize);
        assert!(
            climbed > sustained + 0.2,
            "envelope should climb after retrigger: {climbed} vs {sustained}"
        );
    }

    #[test]
    fn reset_on_retrig_fades_to_zero_without_clicking() {
        let mut adsr = make(AdsrParams {
            attack: time(0.01),
            decay: time(0.1),
            sustain: time(2.0),
            retrigger: Some(gate(0.0)),
            reset_on_retrig: true,
            ..base_params()
        });
        let before = run(&mut adsr, (SR * 0.4) as usize);
        assert!(
            (before - 2.0).abs() < 0.05,
            "should be at sustain, got {before}"
        );

        // Fire the retrigger and watch the trajectory for the next 20ms.
        adsr.params.retrigger = Some(gate(5.0));
        let mut prev = before;
        let mut max_step = 0.0f32;
        let mut reached_zero = false;
        for _ in 0..((SR * 0.02) as usize) {
            adsr.update(SR);
            let v = adsr.outputs.sample.get(0);
            max_step = max_step.max((v - prev).abs());
            prev = v;
            if v < 0.05 {
                reached_zero = true;
            }
        }

        // The old hard reset dropped ~2V in a single sample; the fade keeps every
        // step small (no click).
        assert!(
            max_step < 0.2,
            "reset should be gradual, largest single-sample step was {max_step}"
        );
        // It still brings the level down to (near) zero as part of the reset.
        assert!(
            reached_zero,
            "reset should fade the level down to near zero"
        );
    }

    #[test]
    fn curve_changes_attack_trajectory_but_not_endpoints() {
        // A long attack so the midpoint is well inside the ramp. Linear, positive
        // (log attack), and negative (exp attack) curves should all start at 0 and
        // reach 5V, but the midpoint level should differ: a log attack rises faster
        // early (above linear), an exp attack rises slower early (below linear).
        let halfway = (SR * 0.5) as usize; // halfway through a 1s attack
        let attack_time = || time(1.0);

        let mut linear = make(AdsrParams {
            attack: attack_time(),
            curve: Some(gate(0.0)),
            ..base_params()
        });
        let lin_mid = run(&mut linear, halfway);

        let mut log_attack = make(AdsrParams {
            attack: attack_time(),
            curve: Some(gate(5.0)),
            ..base_params()
        });
        let log_mid = run(&mut log_attack, halfway);

        let mut exp_attack = make(AdsrParams {
            attack: attack_time(),
            curve: Some(gate(-5.0)),
            ..base_params()
        });
        let exp_mid = run(&mut exp_attack, halfway);

        assert!(
            log_mid > lin_mid + 0.5,
            "log attack should be ahead of linear at the midpoint: log={log_mid} lin={lin_mid}"
        );
        assert!(
            exp_mid < lin_mid - 0.5,
            "exp attack should lag linear at the midpoint: exp={exp_mid} lin={lin_mid}"
        );

        // All three converge to 5V by the end of the attack.
        for adsr in [&mut linear, &mut log_attack, &mut exp_attack] {
            let end = run(adsr, halfway + 16);
            assert!(end > 4.99, "attack should complete at 5V, got {end}");
        }
    }

    #[test]
    fn steep_curve_decay_is_smooth() {
        // At curve = 5 the decay is fully exponential. A power curve with an
        // exponent below 1 has infinite slope at the segment start and dumps a
        // large step on the first decay sample (the click). The exp-based curve
        // has finite slope, so every step stays small.
        let mut adsr = make(AdsrParams {
            attack: time(0.001),
            decay: time(0.2),
            sustain: time(0.0), // decay all the way down so the whole fall is exercised
            curve: Some(gate(5.0)),
            ..base_params()
        });
        // Finish the (1ms) attack and settle into the decay.
        run(&mut adsr, (SR * 0.003) as usize);

        let mut prev = adsr.outputs.sample.get(0);
        let mut max_step = 0.0f32;
        for _ in 0..((SR * 0.2) as usize) {
            adsr.update(SR);
            let v = adsr.outputs.sample.get(0);
            max_step = max_step.max((v - prev).abs());
            prev = v;
        }
        // The old power curve dropped well over 0.5V on the first decay sample.
        assert!(
            max_step < 0.1,
            "steep decay should be click-free, largest single-sample step was {max_step}V"
        );
    }

    #[test]
    fn curved_attack_onset_is_gentle() {
        // At curve = 5 the attack is logarithmic (front-loaded), and a 1ms attack
        // would otherwise jump ~0.4V on its first sample — a click on every hit
        // from silence. The per-sample step cap (MAX_LEVEL_STEP) holds the onset to
        // ~0.1V regardless of attack time or sample rate.
        let mut adsr = make(AdsrParams {
            attack: time(0.001),
            decay: time(1.0),
            sustain: time(0.0),
            curve: Some(gate(5.0)),
            ..base_params()
        });
        adsr.update(SR); // first sample of the attack, rising from silence
        let first = adsr.outputs.sample.get(0);
        assert!(
            first <= MAX_LEVEL_STEP * 5.0 + 1e-4,
            "curved attack onset should be capped at ~0.1V, got {first}V"
        );
    }

    #[test]
    fn zero_attack_slews_to_full_level() {
        // A zero attack must still honor the MAX_LEVEL_STEP slew: the envelope
        // reaches 5V over ~1/MAX_LEVEL_STEP samples, never jumping in one.
        let mut adsr = make(AdsrParams {
            attack: time(0.0),
            ..base_params()
        });
        let mut prev = 0.0f32;
        let mut max_step = 0.0f32;
        let mut reached_top_at = None;
        for n in 0..200 {
            adsr.update(SR);
            let v = adsr.outputs.sample.get(0);
            max_step = max_step.max((v - prev).abs());
            prev = v;
            if reached_top_at.is_none() && v > 4.99 {
                reached_top_at = Some(n);
            }
        }
        assert!(
            max_step <= MAX_LEVEL_STEP * 5.0 + 1e-4,
            "zero attack must slew, largest single-sample step was {max_step}V"
        );
        let reached = reached_top_at.expect("envelope should reach 5V");
        assert!(
            reached > 10,
            "the top should arrive over the slew, not in one sample (got sample {reached})"
        );
    }

    #[test]
    fn zero_release_slews_to_zero() {
        // A zero release must ramp a sustained envelope down through the slew
        // instead of snapping 5V→0V in one sample.
        let mut adsr = make(AdsrParams {
            release: time(0.0),
            ..base_params()
        });
        let sustained = run(&mut adsr, (SR * 0.2) as usize);
        assert!(sustained > 4.99, "should sustain near 5V, got {sustained}");

        adsr.params.gate = gate(0.0);
        let mut prev = sustained;
        let mut max_step = 0.0f32;
        let mut reached_zero = false;
        for _ in 0..200 {
            adsr.update(SR);
            let v = adsr.outputs.sample.get(0);
            max_step = max_step.max((v - prev).abs());
            prev = v;
            if v < 0.01 {
                reached_zero = true;
            }
        }
        assert!(
            max_step <= MAX_LEVEL_STEP * 5.0 + 1e-4,
            "zero release must slew, largest single-sample step was {max_step}V"
        );
        assert!(reached_zero, "envelope should reach 0V, ended at {prev}");
    }

    #[test]
    fn onset_is_gentle_across_sample_rates() {
        // The step cap is defined in level, not time, so the first-sample onset
        // stays bounded even at low sample rates where each sample covers more of
        // a short attack.
        for &sr in &[22_050.0_f32, 44_100.0, 48_000.0, 96_000.0] {
            let mut adsr = make(AdsrParams {
                attack: time(0.001),
                curve: Some(gate(5.0)),
                ..base_params()
            });
            adsr.update(sr);
            let first = adsr.outputs.sample.get(0);
            assert!(
                first <= MAX_LEVEL_STEP * 5.0 + 1e-4,
                "onset should stay capped at {sr}Hz, got {first}V"
            );
        }
    }

    #[test]
    fn live_sustain_change_mid_decay_does_not_jump() {
        // Sustain is a live signal; raising it partway through the decay must bend
        // the remaining trajectory smoothly, not snap the level in one sample.
        let mut adsr = make(AdsrParams {
            attack: time(0.001),
            decay: time(0.2),
            sustain: time(0.0),
            curve: Some(gate(0.0)), // linear, so the only motion is the param change
            ..base_params()
        });
        run(&mut adsr, (SR * 0.05) as usize); // settle partway down the decay
        let before = adsr.outputs.sample.get(0);

        adsr.params.sustain = time(4.0); // jump the target up mid-decay
        let mut prev = before;
        let mut max_step = 0.0f32;
        for _ in 0..((SR * 0.05) as usize) {
            adsr.update(SR);
            let v = adsr.outputs.sample.get(0);
            max_step = max_step.max((v - prev).abs());
            prev = v;
        }
        assert!(
            max_step <= MAX_LEVEL_STEP * 5.0 + 1e-4,
            "live sustain change should not jump, largest step was {max_step}V"
        );
    }

    #[test]
    fn live_curve_change_mid_decay_does_not_jump() {
        // Curve is a live signal; sweeping it across a ramp must not discontinuously
        // remap the level.
        let mut adsr = make(AdsrParams {
            attack: time(0.001),
            decay: time(0.3),
            sustain: time(0.0),
            curve: Some(gate(5.0)),
            ..base_params()
        });
        run(&mut adsr, (SR * 0.05) as usize); // partway down the decay

        let mut prev = adsr.outputs.sample.get(0);
        let mut max_step = 0.0f32;
        // Flip the curve from +5 to -5 over the rest of the decay.
        for i in 0..((SR * 0.1) as usize) {
            let t = i as f32 / (SR * 0.1);
            adsr.params.curve = Some(gate(5.0 - 10.0 * t));
            adsr.update(SR);
            let v = adsr.outputs.sample.get(0);
            max_step = max_step.max((v - prev).abs());
            prev = v;
        }
        assert!(
            max_step <= MAX_LEVEL_STEP * 5.0 + 1e-4,
            "live curve change should not jump, largest step was {max_step}V"
        );
    }
}
