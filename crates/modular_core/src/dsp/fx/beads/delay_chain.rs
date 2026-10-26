//! The `$clockedDelay` chain at the hardware rate of the selected quality:
//! each voice records its input plus feedback, reads it back through the
//! controller's taps, mixes dry and wet and limits the result.

use super::buffer::{GrainBuffer, Linear12Codec, Linear16Codec, MuLaw8Codec, Resolution};
use super::chain::{BlockClock, Limiters, Media, POT_LP};
use super::delay_controller::{DelayController, DelayControls, Grid};
use super::feedback::FeedbackPath;
use super::multitap::MultitapDelay;
use super::primitives::{Decimator2, Interpolator2, Ramp};
use super::resample::MonoHistory;
use crate::dsp::utils::SchmittTrigger;

/// Feedback gain at full `feedback`, half the grain engine's, as on the
/// hardware.
const MAX_FEEDBACK_GAIN: f32 = 1.5;
/// Shortest delay in hardware samples.
const MIN_DELAY: f32 = 68.0;

/// Per-voice controls in hardware units, read once per block.
#[derive(Clone, Copy, Debug, Default)]
pub struct DelayVoiceControls {
    pub feedback: f32,
    pub dry_wet: f32,
    /// Base delay time in bars.
    pub bars: f32,
    /// Multiplier, or slice while frozen (0–1).
    pub time: f32,
    pub shape: f32,
    /// How far the windows follow the groove (0–1).
    pub groove_amount: f32,
    /// Playback speed ratio of the repeats.
    pub speed: f32,
    /// Low-pass below 0.5, high-pass above (0–1).
    pub bandwidth: f32,
}

#[derive(Default)]
pub struct DelayVoice {
    pub buffer: GrainBuffer,
    feedback: FeedbackPath,
    multitap: MultitapDelay,
    controller: DelayController,
    speed: Ramp,
    bandwidth: Ramp,

    /// Engine-rate input, read at the hardware rate through the resampler
    /// when the rates differ.
    pub history: MonoHistory,
    /// Hardware-rate input for the current sample.
    pub input: f32,
    pub freeze: bool,
    freeze_schmitt: SchmittTrigger,

    feedback_amount: f32,
    dry_wet: f32,
    groove_amount: f32,
    /// Whether the voice has run a control block. Its first block sets its
    /// controls straight to their targets.
    started: bool,
    dry: Ramp,
    wet: Ramp,
    limiters: Limiters,
    /// Output at the hardware rate, read at the engine rate through the
    /// resampler.
    pub output: MonoHistory,

    /// Clean tape's 2:1 conversion around the delay, and the pair of
    /// hardware samples it processes together.
    decimator: Decimator2,
    interpolator: Interpolator2,
    pair_input: [f32; 2],
    pair_freeze: bool,
    pair_wet: [f32; 2],
    /// Second output of the last pair, emitted on the next hardware sample.
    pair_output: f32,
}

impl DelayVoice {
    /// Allocate the input and output histories. The recording buffer is
    /// handed to the voice separately, so a voice can keep its recording
    /// across patch updates. Main thread only.
    pub fn allocate(&mut self) {
        self.history.allocate();
        self.output.allocate();
    }

    /// Switch to `media`, clearing the recording, taps and histories.
    pub fn configure(&mut self, media: Media) {
        let ratio = media.granular_ratio() as f32;
        let rate = media.sample_rate() / ratio;
        let resolution = media.resolution();
        self.buffer.reset(resolution, media.max_delay());
        self.feedback.reset(rate, resolution, MAX_FEEDBACK_GAIN);
        self.multitap.reset();
        self.controller
            .init(rate, MIN_DELAY / ratio, self.buffer.max_delay() as f32);
        self.history.clear();
        self.output.clear();
        self.limiters.init(media.sample_rate());
        self.decimator = Decimator2::default();
        self.interpolator = Interpolator2::default();
        self.pair_input = [0.0; 2];
        self.pair_freeze = false;
        self.pair_wet = [0.0; 2];
        self.pair_output = 0.0;
    }

    /// Take this engine sample's `input` (hardware units), writing it to the
    /// history at `index` when the resampler `converts`, and its `freeze`
    /// level.
    pub fn sense(&mut self, input: f32, converts: bool, index: u64, freeze: Option<f32>) {
        if converts {
            self.history.write(index, input);
        } else {
            self.input = input;
        }
        self.freeze = match freeze {
            Some(f) => self.freeze_schmitt.process_with_edge(f).0,
            None => false,
        };
    }

    /// Smooth this block's `controls` and set the ramps over `block`
    /// hardware samples (`delay_block` delay samples), with bars lasting
    /// `bar_samples` delay samples.
    fn start_block(
        &mut self,
        controls: DelayVoiceControls,
        bar_samples: f32,
        block: usize,
        delay_block: usize,
    ) {
        if !self.started {
            self.feedback_amount = controls.feedback;
            self.dry_wet = controls.dry_wet;
            self.speed.reset(controls.speed);
            self.bandwidth.reset(controls.bandwidth);
            self.started = true;
        }
        self.feedback_amount += POT_LP * (controls.feedback - self.feedback_amount);
        self.dry_wet += POT_LP * (controls.dry_wet - self.dry_wet);
        self.feedback.start_block(self.feedback_amount, delay_block);
        self.groove_amount = controls.groove_amount;
        let x = self.dry_wet;
        self.dry.start((1.0 - x) * (1.0 + x) * 0.75, block);
        self.wet.start(x * (2.0 - x) * 1.5, block);

        self.speed.start(controls.speed, delay_block);
        self.bandwidth.start(controls.bandwidth, delay_block);
        self.controller.start_block(
            DelayControls {
                interval: controls.bars * bar_samples,
                bars: controls.bars,
                time: controls.time,
                shape: controls.shape,
            },
            delay_block,
        );
    }

    /// Record `input` and return the taps' output for one delay sample.
    #[inline]
    fn render(&mut self, input: f32, freeze: bool, grid: Grid) -> f32 {
        let s = self.feedback.record(input);
        self.buffer.write(s, freeze);
        let taps = self
            .controller
            .render(freeze, grid.grooved(self.groove_amount));
        let speed = self.speed.next();
        let bandwidth = self.bandwidth.next();
        let buffer = &self.buffer;
        let wet = match buffer.resolution() {
            Resolution::MuLaw8 => self
                .multitap
                .render::<MuLaw8Codec>(buffer, &taps, speed, bandwidth),
            Resolution::Linear12 => self
                .multitap
                .render::<Linear12Codec>(buffer, &taps, speed, bandwidth),
            Resolution::Linear16 => self
                .multitap
                .render::<Linear16Codec>(buffer, &taps, speed, bandwidth),
        };
        self.feedback.feed_back(wet);
        wet
    }

    /// Mix hardware sample `k` of the current pair through the limiters.
    #[inline]
    fn finish_sample(&mut self, media: Media, k: usize) -> f32 {
        let dry = self.dry.next();
        let wet = self.wet.next();
        let x = media.colour(self.pair_input[k]) * dry + self.pair_wet[k] * wet;
        self.limiters.process_mono(x)
    }
}

#[derive(Default)]
pub struct DelayChain {
    media: Media,
    clock: BlockClock,
    /// Position within the current pair of hardware samples (clean tape).
    pair_pos: usize,
}

impl DelayChain {
    /// Switch every voice to `media`, clearing the recordings and taps.
    pub fn configure(&mut self, media: Media, voices: &mut [DelayVoice]) {
        self.media = media;
        self.clock.configure(media.sample_rate());
        for voice in voices.iter_mut() {
            voice.configure(media);
        }
        self.pair_pos = 0;
    }

    /// Advance the control-block clock; at a block start, read every voice's
    /// controls and set its ramps. A voice added by a patch update mid-block
    /// starts on its first sample, as its taps need a delay time to render.
    fn begin_sample(
        &mut self,
        voices: &mut [DelayVoice],
        controls: &mut impl FnMut(usize) -> DelayVoiceControls,
        bar_seconds: f32,
    ) {
        let starts = self.clock.tick();
        let ratio = self.media.granular_ratio();
        let block = self.clock.size();
        let bar_samples = bar_seconds * self.media.sample_rate() / ratio as f32;
        for (i, v) in voices.iter_mut().enumerate() {
            if starts || !v.started {
                v.start_block(controls(i), bar_samples, block, block / ratio);
            }
        }
    }

    /// Process one sample at the hardware rate from each voice's `input` and
    /// `freeze`, writing each voice's output to its `output` history at
    /// `index`. Bars last `bar_seconds`, and the taps' windows follow `grid`.
    /// In clean tape the hardware handles each pair of samples together, so
    /// the output is one sample behind the hardware's. `controls` is read
    /// once per control block.
    pub fn process(
        &mut self,
        voices: &mut [DelayVoice],
        controls: &mut impl FnMut(usize) -> DelayVoiceControls,
        grid: Grid,
        bar_seconds: f32,
        index: u64,
    ) {
        let media = self.media;
        if media.granular_ratio() == 1 {
            self.begin_sample(voices, controls, bar_seconds);
            for v in voices.iter_mut() {
                v.pair_input[0] = v.input;
                v.pair_wet[0] = v.render(v.input, v.freeze, grid);
                let x = v.finish_sample(media, 0);
                v.output.write(index, x);
            }
            return;
        }

        // Clean tape: the delay runs at half rate between the hardware's 2×
        // decimation and interpolation filters.
        for v in voices.iter_mut() {
            v.pair_input[self.pair_pos] = v.input;
            v.pair_freeze |= v.freeze;
        }
        if self.pair_pos == 0 {
            self.pair_pos = 1;
            for v in voices.iter_mut() {
                v.output.write(index, v.pair_output);
            }
            return;
        }
        self.pair_pos = 0;

        self.begin_sample(voices, controls, bar_seconds);
        for v in voices.iter_mut() {
            v.decimator.push(v.pair_input[0]);
            v.decimator.push(v.pair_input[1]);
            let low = v.decimator.output();
            let wet = v.render(low, v.pair_freeze, grid);
            v.pair_wet = v.interpolator.process(wet);
            v.pair_freeze = false;
            let x = v.finish_sample(media, 0);
            v.output.write(index, x);
        }
        self.begin_sample(voices, controls, bar_seconds);
        for v in voices.iter_mut() {
            v.pair_output = v.finish_sample(media, 1);
        }
    }
}
