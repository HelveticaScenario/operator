use std::ops::Range;

use crate::{
    dsp::{
        consts::{LUT_SINE, LUT_SINE_SIZE},
        oscillators::{FmMode, apply_fm, sync_blep, sync_edge_fraction},
        utils::{SchmittTrigger, interpolate, sanitize, wrap_phase},
    },
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct SineOscillatorParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: PolySignal,
    /// FM input signal (pre-scaled by user)
    #[deserr(default)]
    fm: Option<PolySignal>,
    /// FM mode: throughZero (default), lin, or exp
    #[serde(default)]
    #[deserr(default)]
    fm_mode: FmMode,
    /// hard sync source — rising edges reset the oscillator phase
    #[deserr(default)]
    #[block_input(optional)]
    sync: Option<PolySignal>,
    /// phase offset in [0, 1) added to the internal phase before sampling
    #[signal(default = 0.0, range = (0.0, 1.0))]
    #[deserr(default)]
    phase_offset: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SineOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Per-channel oscillator state
#[derive(Default, Clone, Copy)]
struct ChannelState {
    phase: f32,
    /// Edge detector for the sync input.
    sync_schmitt: SchmittTrigger,
    /// Previous sync-input sample, for subsample edge interpolation.
    sync_prev: f32,
    /// PolyBLEP residual carried into the next sample from a sync reset.
    blep_carry: f32,
}

/// A sine wave oscillator.
///
/// ## Example
///
/// ```js
/// $sine('c4').out()
/// ```
#[module(name = "$sine", args(freq), block)]
pub struct SineOscillator {
    outputs: SineOscillatorOutputs,
    channel_state: Box<[ChannelState]>,
    params: SineOscillatorParams,
}

impl ChannelState {
    /// Advance the phase by `increment` (cycles per sample) and return the
    /// next output sample. `sync` is this sample's sync input, if connected.
    #[inline(always)]
    fn step(&mut self, increment: f32, offset: f32, sync: Option<f32>) -> f32 {
        self.phase = wrap_phase(self.phase + increment);

        // Phase offset shifts the read position without altering the
        // accumulator, so it never drifts.
        let read_phase = (self.phase + offset).rem_euclid(1.0);

        // Naive sample at the (pre-reset) phase, plus any residual carried
        // from a sync reset on the previous sample.
        let body = interpolate(LUT_SINE, read_phase, LUT_SINE_SIZE);
        let pending = self.blep_carry;
        self.blep_carry = 0.0;

        // Hard sync: a rising edge resets the phase, with a PolyBLEP placed
        // at the subsample crossing to band-limit the discontinuity.
        let mut now = 0.0;
        if let Some(v) = sync {
            if self.sync_schmitt.process(v) {
                let frac = sync_edge_fraction(self.sync_prev, v);
                self.phase = 0.0;
                let after = interpolate(LUT_SINE, offset.rem_euclid(1.0), LUT_SINE_SIZE);
                let (n, carry) = sync_blep(after - body, frac);
                now = n;
                self.blep_carry = carry;
            }
            self.sync_prev = v;
        }

        (body + pending + now) * 5.0
    }
}

impl SineOscillator {
    fn update(&mut self, sample_rate: f32) {
        let p = &self.params;
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            let pitch = p.freq.get_value(ch);
            let fm = p.fm.value_or(ch, 0.0);
            let increment = apply_fm(pitch, fm, p.fm_mode) / sample_rate;
            let offset = p.phase_offset.value_or(ch, 0.0);
            let sync = p.sync.as_ref().map(|sync| sync.get_value(ch));
            self.outputs
                .sample
                .set(ch, state.step(increment, offset, sync));
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut SineOscillatorBlockOutputs,
        range: Range<usize>,
        sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let SineOscillatorParamsBlockInputs {
            freq,
            fm,
            sync,
            phase_offset: offset,
        } = p.block_inputs(range.clone(), channels, &mut scratch)?;
        // A pitch held for the whole range needs its exponential only once.
        let mut held_increments = [0.0f32; PORT_MAX_CHANNELS];
        let held = freq.is_constant() && fm.is_constant();
        if held {
            let (freq, fm) = (freq.row(range.start), fm.row(range.start));
            for ((increment, &pitch), &fm) in held_increments.iter_mut().zip(freq).zip(fm) {
                *increment = apply_fm(pitch, fm, p.fm_mode) / sample_rate;
            }
        }
        for i in range {
            let (pitch, fm, offset) = (freq.row(i), fm.row(i), offset.row(i));
            let sync = sync.as_ref().map(|sync| sync.row(i));
            let slots = outputs
                .sample
                .slot_mut(i)
                .iter_mut()
                .zip(self.channel_state.iter_mut());
            for (ch, (out, state)) in slots.enumerate() {
                let increment = if held {
                    held_increments[ch]
                } else {
                    apply_fm(pitch[ch], fm[ch], p.fm_mode) / sample_rate
                };
                *out = sanitize(state.step(increment, offset[ch], sync.map(|sync| sync[ch])));
            }
        }
        Some(())
    }
}

message_handlers!(impl SineOscillator {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};

    fn make_sine(fm_volts: f32, fm_mode: FmMode) -> SineOscillator {
        let params = SineOscillatorParams {
            freq: PolySignal::mono(Signal::Volts(0.0)), // C4 ≈ 261 Hz
            fm: Some(PolySignal::mono(Signal::Volts(fm_volts))),
            fm_mode,
            sync: None,
            phase_offset: None,
        };
        let channels = params.freq.channels().max(1);
        let mut outputs = SineOscillatorOutputs::default();
        outputs.set_all_channels(channels);
        SineOscillator {
            params,
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
            channel_state: vec![ChannelState::default(); channels].into_boxed_slice(),
        }
    }

    #[test]
    fn recovers_after_non_finite_frequency() {
        // An exp-FM voltage large enough to overflow voct_to_hz to +inf must
        // not stick in the phase accumulator: once the FM input returns to a
        // normal level, the oscillator sounds again within one cycle.
        let mut osc = make_sine(121.0, FmMode::Exp);
        for _ in 0..8 {
            osc.update(48_000.0);
        }
        assert!(
            osc.channel_state[0].phase.is_finite(),
            "phase {} must stay finite through a non-finite frequency",
            osc.channel_state[0].phase
        );

        osc.params.fm = Some(PolySignal::mono(Signal::Volts(0.0)));
        // C4 period ≈ 183.5 samples: a healthy oscillator reaches a
        // substantial level well within one cycle.
        let recovered = (0..184).any(|_| {
            osc.update(48_000.0);
            osc.outputs.sample.get(0).abs() > 1.0
        });
        assert!(recovered, "output must recover within a cycle");
    }
}
