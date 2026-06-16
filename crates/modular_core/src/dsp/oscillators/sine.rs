use crate::{
    dsp::{
        consts::{LUT_SINE, LUT_SINE_SIZE},
        oscillators::{FmMode, apply_fm, sync_blep, sync_edge_fraction},
        utils::{SchmittTrigger, interpolate},
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
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
#[module(name = "$sine", args(freq))]
pub struct SineOscillator {
    outputs: SineOscillatorOutputs,
    channel_state: Box<[ChannelState]>,
    params: SineOscillatorParams,
}

impl SineOscillator {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            let pitch = self.params.freq.get_value(ch);
            let fm = self.params.fm.value_or(ch, 0.0);
            let frequency = apply_fm(pitch, fm, self.params.fm_mode) / sample_rate;
            state.phase += frequency;
            // Wrap phase to [0, 1) — supports negative increments (through-zero FM)
            state.phase = state.phase.rem_euclid(1.0);

            // Phase offset shifts the read position without altering the
            // accumulator, so it never drifts.
            let offset = self.params.phase_offset.value_or(ch, 0.0);
            let read_phase = (state.phase + offset).rem_euclid(1.0);

            // Naive sample at the (pre-reset) phase, plus any residual carried
            // from a sync reset on the previous sample.
            let body = interpolate(LUT_SINE, read_phase, LUT_SINE_SIZE);
            let pending = state.blep_carry;
            state.blep_carry = 0.0;

            // Hard sync: a rising edge resets the phase, with a PolyBLEP placed
            // at the subsample crossing to band-limit the discontinuity.
            let mut now = 0.0;
            if let Some(sync) = &self.params.sync {
                let v = sync.get_value(ch);
                if state.sync_schmitt.process(v) {
                    let frac = sync_edge_fraction(state.sync_prev, v);
                    state.phase = 0.0;
                    let after = interpolate(LUT_SINE, offset.rem_euclid(1.0), LUT_SINE_SIZE);
                    let (n, carry) = sync_blep(after - body, frac);
                    now = n;
                    state.blep_carry = carry;
                }
                state.sync_prev = v;
            }

            self.outputs.sample.set(ch, (body + pending + now) * 5.0);
        }
    }
}

message_handlers!(impl SineOscillator {});
