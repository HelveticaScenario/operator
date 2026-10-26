use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{GATE_HIGH_VOLTAGE, GATE_LOW_VOLTAGE, SchmittTrigger};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct CoinFlipParams {
    /// triggers or gates to flip
    #[signal(type = trig, range = (0.0, 5.0))]
    input: PolySignal,
    /// chance of heads: 0 = never, 2.5 = half the time, 5 = always
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    probability: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CoinFlipOutputs {
    #[output("heads", "triggers that land heads", default)]
    heads: PolyOutput,
    #[output("tails", "triggers that land tails")]
    tails: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct CoinFlipChannelState {
    rng: LcgRng,
    seeded: bool,
    input: SchmittTrigger,
    /// Outcome of the latest flip; routes the input gate until the next edge.
    heads: bool,
}

impl CoinFlipChannelState {
    /// Advance one sample and return the `(heads, tails)` voltages.
    /// `probability` is read only on a rising edge.
    #[inline]
    fn step(&mut self, input: f32, probability: impl FnOnce() -> f32) -> (f32, f32) {
        let (high, edge) = self.input.process_with_edge(input);
        if edge.is_rising() {
            let chance = probability().clamp(0.0, 5.0) / 5.0;
            // `next_unit` can round up to exactly 1.0, so certainty is checked
            // explicitly.
            self.heads = chance >= 1.0 || self.rng.next_unit() < chance;
        }
        let gate = |on: bool| {
            if on {
                GATE_HIGH_VOLTAGE
            } else {
                GATE_LOW_VOLTAGE
            }
        };
        (gate(high && self.heads), gate(high && !self.heads))
    }
}

/// Sends each trigger to heads or tails at random.
///
/// Each trigger on **input** flips a weighted coin, and **probability** sets
/// the chance of heads. The module itself is the heads output; `.tails`
/// carries the rest. Gates keep their length, and each channel flips on its
/// own.
///
/// Place it after `$clockDivider` to turn a steady clock into a loose rhythm,
/// or use both outputs to split one stream between two sounds.
///
/// ```js
/// // Each beat lands on the kick or the snare, 75/25:
/// const flip = $coinFlip($clock.beatTrigger, 3.75)
/// $tr808.kick(flip).out()
/// $tr808.snare(flip.tails).out()
/// ```
#[module(name = "$coinFlip", patch_update, args(input, probability))]
pub struct CoinFlip {
    outputs: CoinFlipOutputs,
    params: CoinFlipParams,
    channel_state: Box<[CoinFlipChannelState]>,
}

impl CoinFlip {
    fn update(&mut self, _sample_rate: f32) {
        for ch in 0..self.channel_count() {
            let (heads, tails) = self.channel_state[ch]
                .step(self.params.input.get_value(ch), || {
                    self.params.probability.value_or(ch, 2.5)
                });
            self.outputs.heads.set(ch, heads);
            self.outputs.tails.set(ch, tails);
        }
    }
}

impl crate::types::PatchUpdateHandler for CoinFlip {
    /// Seeds each unseeded channel off the module's stable heap address. The
    /// per-channel `seeded` flag rides `transfer_state_from`, so a running
    /// channel keeps its sequence across patch updates.
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            if !state.seeded {
                state.rng.seed(base, ch);
                state.seeded = true;
            }
        }
    }
}

message_handlers!(impl CoinFlip {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, PatchUpdateHandler, Signal};

    const SR: f32 = 48_000.0;

    fn volts(v: f32) -> PolySignal {
        PolySignal::mono(Signal::Volts(v))
    }

    fn make(params: CoinFlipParams) -> CoinFlip {
        let channels = __coin_flip_derive_channel_count(&params);
        let mut outputs = CoinFlipOutputs::default();
        outputs.set_all_channels(channels);
        let mut m = CoinFlip {
            params,
            outputs,
            channel_state: vec![CoinFlipChannelState::default(); channels].into_boxed_slice(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        crate::dsp::utils::rng::TEST_SEED_BASE.set(Some(0x5eed));
        m.on_patch_update();
        crate::dsp::utils::rng::TEST_SEED_BASE.set(None);
        m
    }

    /// Fire `n` two-sample triggers, each followed by two low samples, and
    /// return per-channel counts of `(heads, tails)` rising edges.
    fn flip(m: &mut CoinFlip, n: usize) -> Vec<(usize, usize)> {
        let channels = m.channel_count();
        let mut counts = vec![(0, 0); channels];
        let mut prev = vec![(0.0, 0.0); channels];
        for i in 0..n * 4 {
            m.params.input = volts(if i % 4 < 2 { 5.0 } else { 0.0 });
            m.update(SR);
            for ch in 0..channels {
                let now = (m.outputs.heads.get(ch), m.outputs.tails.get(ch));
                assert!(
                    now.0 == 0.0 || now.1 == 0.0,
                    "both outputs high on channel {ch}"
                );
                counts[ch].0 += usize::from(now.0 > prev[ch].0);
                counts[ch].1 += usize::from(now.1 > prev[ch].1);
                prev[ch] = now;
            }
        }
        counts
    }

    fn params(probability: PolySignal) -> CoinFlipParams {
        CoinFlipParams {
            input: volts(0.0),
            probability: Some(probability),
        }
    }

    #[test]
    fn every_trigger_lands_on_exactly_one_side() {
        let mut m = make(params(volts(2.5)));
        let [(heads, tails)] = flip(&mut m, 1000)[..] else {
            unreachable!()
        };
        assert_eq!(heads + tails, 1000);
        assert!((400..600).contains(&heads), "{heads} heads of 1000");
    }

    #[test]
    fn probability_endpoints_are_certain() {
        let mut never = make(params(volts(0.0)));
        assert_eq!(flip(&mut never, 500), [(0, 500)]);
        let mut always = make(params(volts(5.0)));
        assert_eq!(flip(&mut always, 500), [(500, 0)]);
    }

    #[test]
    fn probability_scales_heads_rate() {
        let mut m = make(params(volts(1.0)));
        let [(heads, _)] = flip(&mut m, 2000)[..] else {
            unreachable!()
        };
        assert!((320..480).contains(&heads), "{heads} heads of 2000");
    }

    #[test]
    fn default_probability_is_even() {
        let mut m = make(CoinFlipParams {
            input: volts(0.0),
            probability: None,
        });
        let [(heads, _)] = flip(&mut m, 1000)[..] else {
            unreachable!()
        };
        assert!((400..600).contains(&heads), "{heads} heads of 1000");
    }

    #[test]
    fn heads_follows_input_gate_length() {
        let mut m = make(params(volts(5.0)));
        m.params.input = volts(5.0);
        for _ in 0..100 {
            m.update(SR);
            assert_eq!(m.outputs.heads.get(0), GATE_HIGH_VOLTAGE);
        }
        m.params.input = volts(0.0);
        m.update(SR);
        assert_eq!(m.outputs.heads.get(0), GATE_LOW_VOLTAGE);
    }

    #[test]
    fn channels_flip_independently() {
        let mut m = make(params(PolySignal::poly(&vec![Signal::Volts(2.5); 4])));
        let channels = m.channel_count();
        let mut outcomes = vec![Vec::new(); channels];
        for _ in 0..64 {
            for (ch, &(heads, _)) in flip(&mut m, 1).iter().enumerate() {
                outcomes[ch].push(heads);
            }
        }
        for (a, b) in (0..channels).flat_map(|a| (a + 1..channels).map(move |b| (a, b))) {
            assert_ne!(outcomes[a], outcomes[b], "channels {a} and {b} match");
        }
    }
}
