use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{Decay, Svf};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{SchmittTrigger, voct_to_hz};
use crate::poly::{MonoSignal, MonoSignalExt, PolyOutput};
use std::f32::consts::{FRAC_PI_4, SQRT_2};

/// ≈D#6 (1.24 kHz) in V/Oct.
const DEFAULT_FREQ: f32 = 2.25;
const DEFAULT_DECAY: f32 = 0.255;
const DEFAULT_TAIL: f32 = 3.25;
const DEFAULT_SLOPPY: f32 = 1.2;
const DEFAULT_SPREAD: f32 = 3.8;
const DEFAULT_TONE: f32 = 1.6;

/// The claps land at `T` and `2T` after the strike, with
/// `T = CLAP_BASE_SECS + CLAP_SLOPPY_SECS · sloppy` (sloppy 0–1); the second
/// is `SECOND_CLAP` times the first.
const CLAP_BASE_SECS: f32 = 0.012;
const CLAP_SLOPPY_SECS: f32 = 0.087;
const SECOND_CLAP: f32 = 0.75;
/// The tail starts at the strike, flickering at random every `FLICKER_SECS`
/// until the first clap lands.
const TAIL_LEVEL: f32 = 0.9;
const FLICKER_SECS: f32 = 0.0015;
/// The tail, flicker and all, takes a narrower band than the claps: its noise
/// is band-passed twice at `freq`, the second time with this resonance.
const TAIL_Q: f32 = 0.7;
/// The tail falls to −60 dB over `TAIL_MIN_SECS + TAIL_SECS · tail^TAIL_CURVE`
/// seconds (tail 0–1). Below a quarter of the range it also quietens, to
/// `TAIL_MIN_LEVEL` of full at 0.
const TAIL_MIN_SECS: f32 = 0.15;
const TAIL_SECS: f32 = 0.69;
const TAIL_CURVE: f32 = 1.6;
const TAIL_MIN_LEVEL: f32 = 0.3;
/// The noise is band-passed around `freq`, then low-passed at
/// `TOP_RATIO` times `freq` to steepen the upper skirt; `tone` mixes in highs.
const BAND_Q: f32 = 1.6;
const TOP_RATIO: f32 = 2.5;
const TONE_HIGHS: f32 = 0.05;
const NOISE_GAIN: f32 = 4.5;
/// Each clap lands at a random pan, up to `PAN_RANGE` of the way to one side
/// at full `spread`, leaning outwards by `PAN_CURVE`; the second lands on the
/// opposite side to the first. The tail stays centred.
const PAN_RANGE: f32 = 1.0;
const PAN_CURVE: f32 = 0.7;
/// Output scale that puts a default hit's peak near full scale.
const OUTPUT_GAIN: f32 = 0.4;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ClapParams {
    /// rising edges start a clap
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: MonoSignal,
    /// centre of the clap's band in V/Oct (default ≈D#6, 1.24 kHz)
    #[signal(type = pitch, default = 2.25, range = (-0.5, 3.6))]
    #[deserr(default)]
    freq: Option<MonoSignal>,
    /// seconds for each clap to fall to −60 dB (default 0.255)
    #[signal(default = 0.255, range = (0.0, 2.0))]
    #[deserr(default)]
    decay: Option<MonoSignal>,
    /// length of the noisy tail around the claps, 0–5 V: 0.15 s up to 0.84 s
    /// to fall to −60 dB, and quieter near 0 (default 3.25)
    #[signal(default = 3.25, range = (0.0, 5.0))]
    #[deserr(default)]
    tail: Option<MonoSignal>,
    /// how late the claps land, 0–5 V: the first at 12–99 ms, the second
    /// twice as late (default 1.2)
    #[signal(default = 1.2, range = (0.0, 5.0))]
    #[deserr(default)]
    sloppy: Option<MonoSignal>,
    /// stereo width, 0–5 V: how far each clap may land to either side; 0 is
    /// mono (default 3.8)
    #[signal(default = 3.8, range = (0.0, 5.0))]
    #[deserr(default)]
    spread: Option<MonoSignal>,
    /// brightness, 0–5 V (default 1.6)
    #[signal(default = 1.6, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<MonoSignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClapOutputs {
    /// Stereo output (left on channel 0, right on channel 1).
    #[output("output", "stereo clap output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default)]
struct ClapState {
    trigger: SchmittTrigger,
    claps: Decay,
    tail: Decay,
    /// Seconds since the strike, and when the claps land.
    elapsed: f32,
    clap_time: f32,
    landed: usize,
    /// Random level of the tail before the first clap, and seconds until it
    /// next changes.
    flicker: f32,
    flicker_wait: f32,
    /// Left and right gains of the latest clap, and which side it went to.
    pan: [f32; 2],
    side: f32,
    rng: LcgRng,
    /// Randomness for the flicker and the pans.
    jitter: LcgRng,
    band: Svf,
    top: Svf,
    tail_band: Svf,
    tail_narrow: Svf,
    sample_period: f32,
    seeded: bool,
}

/// Hand clap: two claps over a flickering, reverberant noise tail, in stereo.
///
/// Each rising edge on **trigger** starts a flickering noisy tail, then lands
/// two claps, the first 12–99 ms later as **sloppy** rises and the second
/// twice as late, each ringing for **decay** seconds. The tail rings on under
/// them, lasting longer as **tail** rises. Everything is band-passed around **freq**, brightened by
/// **tone** and widened by **spread**.
///
/// Output is stereo (left on channel 0, right on channel 1), range **±5 V**.
///
/// ```js
/// $clap($clock.beatTrigger, { tail: 1.5 }).out()
/// ```
#[module(name = "$clap", channels = 2, has_init, patch_update, args(trigger))]
pub struct Clap {
    outputs: ClapOutputs,
    params: ClapParams,
    state: ClapState,
}

impl Clap {
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_period = 1.0 / sample_rate;
        self.state.landed = 2;
        self.state.pan = [1.0, 1.0];
    }

    fn update(&mut self, sample_rate: f32) {
        let p = &self.params;
        let st = &mut self.state;

        if st.trigger.process(p.trigger.get_value()) {
            st.tail.strike();
            st.elapsed = 0.0;
            let sloppy = p.sloppy.value_or(DEFAULT_SLOPPY).clamp(0.0, 5.0) / 5.0;
            st.clap_time = CLAP_BASE_SECS + CLAP_SLOPPY_SECS * sloppy;
            st.landed = 0;
            st.flicker_wait = 0.0;
        }
        let pending = st.landed < 2;
        if !st.claps.active() && !st.tail.active() && !pending {
            self.outputs.sample.set(0, 0.0);
            self.outputs.sample.set(1, 0.0);
            return;
        }

        let mut flicker = 1.0;
        if pending {
            if st.elapsed >= st.clap_time * (st.landed + 1) as f32 {
                st.claps.strike();
                let spread = p.spread.value_or(DEFAULT_SPREAD).clamp(0.0, 5.0) / 5.0;
                let side = if st.landed == 0 {
                    st.jitter.next_bipolar().signum()
                } else {
                    -st.side
                };
                st.side = side;
                let reach = st.jitter.next_unit().powf(PAN_CURVE);
                let angle = FRAC_PI_4 * (1.0 + PAN_RANGE * spread * side * reach);
                st.pan = [SQRT_2 * angle.cos(), SQRT_2 * angle.sin()];
                if st.landed == 1 {
                    st.claps.level = SECOND_CLAP;
                }
                st.landed += 1;
            }
            if st.landed == 0 {
                st.flicker_wait -= st.sample_period;
                if st.flicker_wait <= 0.0 {
                    st.flicker = st.jitter.next_unit();
                    st.flicker_wait += FLICKER_SECS;
                }
                flicker = st.flicker;
            }
        }
        st.elapsed += st.sample_period;

        let claps = st.claps.next(p.decay.value_or(DEFAULT_DECAY), sample_rate);
        let tail_amount = p.tail.value_or(DEFAULT_TAIL).clamp(0.0, 5.0) / 5.0;
        let tail_t60 = TAIL_MIN_SECS + TAIL_SECS * tail_amount.powf(TAIL_CURVE);
        let tail = st.tail.next(tail_t60, sample_rate);
        let tail_level = TAIL_MIN_LEVEL + (1.0 - TAIL_MIN_LEVEL) * (4.0 * tail_amount).min(1.0);
        let tail = TAIL_LEVEL * tail_level * flicker * tail;

        let noise = st.rng.next_bipolar();
        let hz = voct_to_hz(p.freq.value_or(DEFAULT_FREQ));
        let tone = p.tone.value_or(DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
        let out = st.band.process(noise, hz, BAND_Q, sample_rate);
        let shaped = st
            .top
            .process(out.bp, TOP_RATIO * hz, 0.707, sample_rate)
            .lp;
        let clap = claps * (shaped + TONE_HIGHS * tone * out.hp);
        let banded = st
            .tail_band
            .process(tail * noise, hz, BAND_Q, sample_rate)
            .bp;
        let narrow = st.tail_narrow.process(banded, hz, TAIL_Q, sample_rate).bp;
        let gain = NOISE_GAIN * OUTPUT_GAIN * 5.0;
        for ch in 0..2 {
            self.outputs
                .sample
                .set(ch, (st.pan[ch] * clap + narrow) * gain);
        }
    }
}

impl crate::types::PatchUpdateHandler for Clap {
    /// Seeds the noise and jitter streams off this module's stable heap
    /// address once, so they carry over unchanged across patch updates.
    fn on_patch_update(&mut self) {
        if !self.state.seeded {
            let base = seed_base(self);
            self.state.rng.seed(base, 0);
            self.state.jitter.seed(base, 1);
            self.state.seeded = true;
        }
    }
}

message_handlers!(impl Clap {});
