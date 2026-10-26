//! `$clockedDelay`: Beads' delay mode, timed in bars of the playhead.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::{sanitize, semitones_to_ratio};
use crate::poly::{MonoSignal, PolyOutput, PolySignal, PolySignalExt};
use crate::types::PatchUpdateHandler;

use super::chain::Media;
use super::delay_chain::{DelayChain, DelayVoice, DelayVoiceControls};
use super::delay_controller::Grid;
use super::fresh::FreshBuffers;
use super::resample::Resampler;
use super::wav_load::silent_buffer;
use super::{FULL_SCALE_VOLTS, GranularQuality, input_value, unit};

/// Engine samples over which the playhead's speed is measured.
const TEMPO_WINDOW: u32 = 256;
/// Tempo and meter when `bpm` and `barLength` are unconnected: the clock's
/// defaults.
const DEFAULT_BPM: f32 = 120.0;
const DEFAULT_BAR_LENGTH: f32 = 4.0;
/// Shortest `bars`, keeping the window period positive.
const MIN_BARS: f32 = 1e-3;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ClockedDelayParams {
    /// audio to delay; each channel gets its own delay
    input: PolySignal,
    /// Transport playhead in bars, setting the tempo and the phase of the
    /// repeats' windows.
    #[default_connection(module = RootClock, port = "playhead", channels = [0, 1])]
    #[deserr(skip)]
    #[schemars(skip)]
    playhead: Option<MonoSignal>,
    /// Transport tempo in BPM, timing the delay until the playhead moves.
    #[default_connection(module = RootClock, port = "tempo", channel = 0)]
    #[deserr(skip)]
    #[schemars(skip)]
    bpm: Option<MonoSignal>,
    /// Transport bar length in quarter notes, with `bpm` timing the delay
    /// until the playhead moves.
    #[default_connection(module = RootClock, port = "barLength", channel = 0)]
    #[deserr(skip)]
    #[schemars(skip)]
    bar_length: Option<MonoSignal>,
    /// Global groove position minus the playhead, in bars, which
    /// `grooveAmount` applies to the repeats' windows.
    #[default_connection(module = RootClock, port = "grooveOffset", channel = 0)]
    #[deserr(skip)]
    #[schemars(skip)]
    groove: Option<MonoSignal>,
    /// base delay time in bars (default 0.25)
    #[signal(default = 0.25, range = (0.0, 4.0))]
    #[deserr(default)]
    bars: Option<PolySignal>,
    /// multiplies the base delay time from ×1 at 0 to ×16 at 5; while
    /// frozen, picks the slice of the held audio to loop instead (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    time: Option<PolySignal>,
    /// window over each repeat: 0 = none, 1 = sharp attack, 2.5 = triangle,
    /// 5 = ramp up (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    shape: Option<PolySignal>,
    /// how far the `shape` windows follow the global groove: 0 = straight,
    /// 5 = fully grooved; the repeats themselves stay straight (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    groove_amount: Option<PolySignal>,
    /// while high, recording stops and the delay loops slices of the audio
    /// it holds
    #[signal(type = gate, range = (0.0, 5.0))]
    #[deserr(default)]
    freeze: Option<PolySignal>,
    /// pitch of the repeats; they play at the input's own pitch when `freq`
    /// equals `rootFreq` (default C4)
    #[signal(type = pitch)]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// pitch of the input, so `freq` shifts the repeats in tune (default C4)
    #[signal(type = pitch)]
    #[deserr(default)]
    root_freq: Option<PolySignal>,
    /// filters the repeats: below 2.5 low-passes, above 2.5 high-passes
    /// (default 2.5, unfiltered)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// feeds the repeats back into the delay, for more of them (0–5,
    /// default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    feedback: Option<PolySignal>,
    /// dry/wet: 0 = input only, 5 = repeats only (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    mix: Option<PolySignal>,
    /// recording character: digital, vintage, cleanTape or dirtyTape
    /// (default digital)
    #[serde(default)]
    #[deserr(default)]
    quality: GranularQuality,
    /// A silent recording buffer per voice at `quality`, made by `init`.
    #[serde(skip)]
    #[deserr(skip)]
    #[schemars(skip)]
    fresh: Option<FreshBuffers>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClockedDelayOutputs {
    #[output("output", "delayed signal, one channel per delay", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Measures the playhead's speed, so delay times follow the tempo.
#[derive(Default)]
struct TempoTracker {
    started: bool,
    last: f64,
    window_start: f64,
    count: u32,
    /// Bars per engine sample of the last window; 0 before one is measured.
    candidate: f64,
    /// Accepted bars per engine sample; 0 until the playhead first moves,
    /// while bars follow `bpm` and `barLength`.
    speed: f64,
    /// Whether the playhead moved on the last sample.
    moving: bool,
}

impl TempoTracker {
    fn update(&mut self, position: f64) {
        if !self.started {
            self.started = true;
            self.last = position;
            self.window_start = position;
            return;
        }
        let moving = position != self.last;
        self.last = position;
        if moving && !self.moving {
            // Measure from the first moving sample, so no window spans the
            // playhead starting, nor a jump as it starts.
            self.moving = true;
            self.window_start = position;
            self.count = 0;
            return;
        }
        self.moving = moving;
        self.count += 1;
        if self.count < TEMPO_WINDOW {
            return;
        }
        let mut distance = position - self.window_start;
        // A playhead without a bar count wraps from 1 back to 0.
        if distance < -0.5 {
            distance += 1.0;
        }
        self.window_start = position;
        self.count = 0;
        if distance <= 0.0 {
            return;
        }
        // A jump of the playhead upsets one window; two windows agreeing
        // confirm a new tempo.
        let speed = distance / TEMPO_WINDOW as f64;
        if self.speed == 0.0 || (speed - self.candidate).abs() <= 0.01 * speed {
            self.speed = speed;
        }
        self.candidate = speed;
    }

    /// Seconds per bar, from the measured speed or else from `bpm` and
    /// `bar_length` quarter notes.
    fn bar_seconds(&self, sample_rate: f32, bpm: f32, bar_length: f32) -> f32 {
        if self.speed > 0.0 {
            (1.0 / (self.speed * sample_rate as f64)) as f32
        } else {
            bar_length.max(0.01) * 60.0 / bpm.max(1.0)
        }
    }
}

#[derive(Default)]
struct ClockedDelayState {
    engine_sample_rate: f32,
    media: Media,
    chain: DelayChain,
    resampler: Resampler,
    tempo: TempoTracker,
}

/// Tempo-synced delay from Mutable Instruments Beads' delay mode. Its repeats
/// come from feedback inside the module, so it needs no feedback loop in the
/// patch.
///
/// The delay time is `bars` of the playhead, which follows the transport,
/// times a multiplier set by `time`. When the time or the tempo changes, the
/// old repeats crossfade into the new ones at an unchanged pitch.
/// `shape` windows each repeat with an envelope one delay time long, locked
/// to the playhead, so the repeats come back as rhythmic slices;
/// `grooveAmount` swings the windows with the global groove. `freq`
/// shifts their pitch against `rootFreq`, and `tone` filters them. While
/// `freeze` is high the delay stops recording and loops slices of what it
/// holds, one base delay time long; `time` then picks the slice.
///
/// Each input channel gets its own delay. Follow it with `$wowFlutter` and
/// `$reverb` for the rest of Beads' sound.
///
/// ```js
/// const pluck = $saw('c3').amplitude($perc($clock.beatTrigger, { decay: 0.1 }))
/// $clockedDelay(pluck, { bars: 3 / 16, feedback: 3 }).out()
/// ```
#[module(name = "$clockedDelay", has_init, patch_update, args(input))]
pub struct ClockedDelay {
    outputs: ClockedDelayOutputs,
    state: ClockedDelayState,
    params: ClockedDelayParams,
    /// One delay per channel of the widest input.
    channel_state: Box<[DelayVoice]>,
}

impl ClockedDelay {
    /// Allocate every voice and a fresh recording buffer for each, and
    /// configure for the initial quality. Until `on_patch_update` hands them
    /// their buffers, the voices share a silent one. Main thread.
    fn init(&mut self, sample_rate: f32) {
        super::primitives::warm_tables();
        let media = self.params.quality.media();
        let silence = silent_buffer(media);
        for voice in self.channel_state.iter_mut() {
            voice.allocate();
            voice.buffer = silence.clone();
        }
        self.params.fresh = Some(FreshBuffers::allocate(self.channel_state.len(), media));
        self.state.engine_sample_rate = sample_rate;
        self.state.resampler.allocate(sample_rate);
        self.configure(media);
    }

    fn configure(&mut self, media: Media) {
        let state = &mut self.state;
        state.media = media;
        state.chain.configure(media, &mut self.channel_state);
        state
            .resampler
            .configure(state.engine_sample_rate, media.sample_rate());
    }

    fn voice_controls(params: &ClockedDelayParams, voice: usize) -> DelayVoiceControls {
        let p = params;
        let freq = p.freq.value_or(voice, 0.0);
        let root = p.root_freq.value_or(voice, 0.0);
        let semitones = sanitize((freq - root) * 12.0).clamp(-84.0, 84.0);
        DelayVoiceControls {
            feedback: unit(p.feedback.value_or(voice, 2.5)),
            dry_wet: unit(p.mix.value_or(voice, 2.5)),
            bars: sanitize(p.bars.value_or(voice, 0.25)).max(MIN_BARS),
            time: unit(p.time.value_or(voice, 0.0)),
            shape: unit(p.shape.value_or(voice, 0.0)),
            groove_amount: unit(p.groove_amount.value_or(voice, 0.0)),
            speed: semitones_to_ratio(semitones),
            bandwidth: unit(p.tone.value_or(voice, 2.5)),
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let params = &self.params;
        let state = &mut self.state;
        let voices = &mut self.channel_state;

        let position = params
            .playhead
            .as_ref()
            .map_or(0.0, MonoSignal::get_value_f64);
        state.tempo.update(position);
        let grid = Grid {
            position,
            groove: params
                .groove
                .as_ref()
                .map_or(0.0, MonoSignal::get_value_f64),
            moving: state.tempo.moving,
        };
        let bpm = params
            .bpm
            .as_ref()
            .map_or(DEFAULT_BPM, |b| sanitize(b.get_value()));
        let bar_length = params
            .bar_length
            .as_ref()
            .map_or(DEFAULT_BAR_LENGTH, |b| sanitize(b.get_value()));
        let bar_seconds = state
            .tempo
            .bar_seconds(state.engine_sample_rate, bpm, bar_length);

        let index = state.resampler.input_index();
        let converts = state.resampler.converts();
        for (i, v) in voices.iter_mut().enumerate() {
            v.sense(
                input_value(&params.input, i),
                converts,
                index,
                params.freeze.as_ref().map(|f| f.get_value(i)),
            );
        }

        let ClockedDelayState {
            chain, resampler, ..
        } = state;
        let mut controls = |i: usize| Self::voice_controls(params, i);
        let tap = resampler.tick(|inner, out_index| {
            if converts {
                for v in voices.iter_mut() {
                    v.input = inner.read(&v.history);
                }
            }
            chain.process(voices, &mut controls, grid, bar_seconds, out_index);
        });

        for (i, v) in voices.iter().enumerate() {
            self.outputs
                .sample
                .set(i, tap.read(&v.output) * FULL_SCALE_VOLTS);
        }
    }
}

impl PatchUpdateHandler for ClockedDelay {
    fn on_patch_update(&mut self) {
        // A quality change clears every voice, and a voice new to this
        // module still holds the silent stand-in: both take a fresh buffer,
        // which needs no clearing here. Other voices keep recording into
        // their own.
        let media = self.params.quality.media();
        let reconfigure = media != self.state.media;
        if let Some(fresh) = &mut self.params.fresh {
            for (i, v) in self.channel_state.iter_mut().enumerate() {
                if fresh.hand_out(i, &mut v.buffer, reconfigure) && !reconfigure {
                    v.configure(media);
                }
            }
        }
        if reconfigure {
            self.configure(media);
        }
    }
}

message_handlers!(impl ClockedDelay {});

#[cfg(test)]
#[path = "tests_delay.rs"]
mod tests;
