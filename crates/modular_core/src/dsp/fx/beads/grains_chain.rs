//! The `$granular` chain at the hardware rate of the selected quality:
//! each voice's grain engine and dry/wet mix, then limiters both on each
//! voice alone and on the voices placed in stereo.

use super::chain::{BlockClock, Media, POT_LP, voice_place};
use super::granular::GrainStartRequest;
use super::player::{GrainTrigger, bounded_random};
use super::primitives::{Frame, Interpolator2, Limiter};
use super::resample::MonoHistory;
use super::voice::{Voice, VoiceControls};
use crate::dsp::utils::rng::LcgRng;

/// The hardware's pre-reverb and output limiters, for one signal.
#[derive(Clone, Copy, Default)]
struct Limiters {
    pre: Limiter,
    output: Limiter,
}

impl Limiters {
    fn init(&mut self, sample_rate: f32) {
        self.pre.init(1.0 / (sample_rate * 4.0), 0.9);
        self.output.init(1.0 / (sample_rate * 6.0), 0.9);
    }
}

#[derive(Default)]
pub struct GrainsVoice {
    pub voice: Voice,
    limiters: Limiters,
    /// Output at the hardware rate, read at the engine rate through the
    /// resampler.
    pub output: MonoHistory,
    /// Clean tape's 1:2 interpolation of the unpanned grains.
    mono_interpolator: Interpolator2,
    /// Unpanned grain output for each hardware sample of the current pair.
    pair_mono: [f32; 2],
    /// Second output of the last pair, emitted on the next hardware sample
    /// (clean tape).
    pair_output: f32,
}

impl GrainsVoice {
    /// Allocate buffers. Main thread only.
    pub fn allocate(&mut self) {
        self.voice.allocate();
        self.output.allocate();
    }

    /// Allocate a voice that plays a buffer handed to it. Main thread only.
    pub fn allocate_playback(&mut self) {
        self.voice.allocate_playback();
        self.output.allocate();
    }

    fn configure(&mut self, media: Media) {
        self.voice.configure(media);
        self.limiters.init(media.sample_rate());
        self.output.clear();
        self.mono_interpolator = Interpolator2::default();
        self.pair_mono = [0.0; 2];
        self.pair_output = 0.0;
    }
}

#[derive(Default)]
pub struct GrainsChain {
    media: Media,
    clock: BlockClock,
    /// Smoothed stereo width in volts.
    width: f32,
    /// Position within the current pair of hardware samples (clean tape).
    pair_pos: usize,
    stereo_limiters: Limiters,
    /// Left and right stereo output at the hardware rate.
    pub stereo: [MonoHistory; 2],
    stereo_pair_output: Frame,
}

impl GrainsChain {
    /// Allocate buffers. Main thread only.
    pub fn allocate(&mut self) {
        super::primitives::warm_tables();
        for h in self.stereo.iter_mut() {
            h.allocate();
        }
    }

    /// Switch every voice to `media`, clearing the recordings and grains.
    pub fn configure(&mut self, media: Media, voices: &mut [GrainsVoice]) {
        self.media = media;
        self.clock.configure(media.sample_rate());
        for voice in voices.iter_mut() {
            voice.configure(media);
        }
        self.pair_pos = 0;
        self.stereo_limiters.init(media.sample_rate());
        for h in self.stereo.iter_mut() {
            h.clear();
        }
        self.stereo_pair_output = Frame::ZERO;
    }

    /// Queue a grain start for its voice; false when that voice already has
    /// one waiting.
    pub fn offer(&mut self, voices: &mut [GrainsVoice], t: GrainTrigger) -> bool {
        let voice = &mut voices[t.voice].voice;
        if voice.trigger.is_some() {
            return false;
        }
        let fractional_sample = if self.media.granular_ratio() == 2 {
            (t.fractional_sample + self.pair_pos as f32) * 0.5
        } else {
            t.fractional_sample
        };
        voice.trigger = Some(GrainTrigger {
            fractional_sample,
            ..t
        });
        true
    }

    /// Advance the control-block clock; at a block start, read and smooth
    /// every voice's controls and set its ramps.
    fn begin_sample(
        &mut self,
        voices: &mut [GrainsVoice],
        controls: &mut impl FnMut(usize) -> VoiceControls,
        width: &mut impl FnMut() -> f32,
    ) {
        if !self.clock.tick() {
            return;
        }
        let block = self.clock.size();
        let grain_block = block / self.media.granular_ratio();
        self.width += POT_LP * (width() - self.width);
        let n = voices.len();
        for (i, v) in voices.iter_mut().enumerate() {
            v.voice.start_block(controls(i), block, grain_block);
            v.voice.start_place(voice_place(i, n, self.width), block);
        }
    }

    /// Resolve each voice's waiting grain start. A voice's grain levels and
    /// stereo spread follow the number of its own grains sounding.
    fn start_grains(voices: &mut [GrainsVoice], rng: &mut LcgRng) {
        for v in voices.iter_mut() {
            let voice = &mut v.voice;
            if let Some(trigger) = voice.trigger.take() {
                let active = (voice.granular.player.active_count() + 1) as f32;
                let pan = bounded_random(rng, 0.5 * active * trigger.params.spread);
                voice.start = Some(GrainStartRequest {
                    trigger,
                    gain: 1.0 / active.sqrt(),
                    pan,
                });
            }
        }
    }

    /// Run every voice's grain engine for one grain-engine sample.
    fn render(voices: &mut [GrainsVoice], clean_tape: bool) {
        for v in voices.iter_mut() {
            let voice = &mut v.voice;
            let place = voice.place();
            let start = voice.start.take();
            if clean_tape {
                let low = voice.decimator.output();
                let (wet, mono) = voice.granular.process(low, voice.pair_freeze, start, place);
                let l = voice.interpolator[0].process(wet.l);
                let r = voice.interpolator[1].process(wet.r);
                voice.pair_wet = [Frame::new(l[0], r[0]), Frame::new(l[1], r[1])];
                v.pair_mono = v.mono_interpolator.process(mono);
                voice.pair_freeze = false;
            } else {
                let (wet, mono) = voice
                    .granular
                    .process(voice.input, voice.freeze, start, place);
                voice.pair_input[0] = voice.input;
                voice.pair_wet[0] = wet;
                v.pair_mono[0] = mono;
            }
        }
    }

    /// Mix hardware sample `k` of the current pair: hand each voice's own
    /// signal, through its limiters, to `emit`, and return the voices placed
    /// in stereo through their own limiters.
    fn finish_sample(
        &mut self,
        voices: &mut [GrainsVoice],
        k: usize,
        mut emit: impl FnMut(&mut GrainsVoice, f32),
    ) -> Frame {
        let media = self.media;
        let mut stereo = Frame::ZERO;
        for v in voices.iter_mut() {
            let voice = &mut v.voice;
            let place = [voice.place[0].next(), voice.place[1].next()];
            let d = media.colour(voice.pair_input[k]);
            let dry = voice.dry.next();
            let wet = voice.wet.next();

            // Only voices fed by their own input channel carry dry signal
            // into the stereo mix, so a shared channel is not doubled.
            let ds = if voice.dry_source { d } else { 0.0 };
            stereo += Frame::new(ds * place[0], ds * place[1]) * dry + voice.pair_wet[k] * wet;

            let mut x = d * dry + v.pair_mono[k] * wet;
            x = v.limiters.pre.process_mono(x);
            x = v.limiters.output.process_mono(x);
            emit(v, x);
        }

        let stereo = self.stereo_limiters.pre.process(stereo);
        self.stereo_limiters.output.process(stereo)
    }

    fn write_stereo(&mut self, index: u64, x: Frame) {
        self.stereo[0].write(index, x.l);
        self.stereo[1].write(index, x.r);
    }

    /// Process one sample at the hardware rate from each voice's `input`,
    /// `freeze` and waiting grain start, writing each voice's output to its
    /// `output` history and the stereo mix to `stereo` at `index`. In clean
    /// tape the hardware handles each pair of samples together, so the output
    /// is one sample behind the hardware's. `controls` and `width` are read
    /// once per control block.
    pub fn process(
        &mut self,
        voices: &mut [GrainsVoice],
        controls: &mut impl FnMut(usize) -> VoiceControls,
        width: &mut impl FnMut() -> f32,
        rng: &mut LcgRng,
        index: u64,
    ) {
        if self.media.granular_ratio() == 1 {
            self.begin_sample(voices, controls, width);
            Self::start_grains(voices, rng);
            Self::render(voices, false);
            let stereo = self.finish_sample(voices, 0, |v, x| v.output.write(index, x));
            self.write_stereo(index, stereo);
            return;
        }

        // Clean tape: the grain engine runs at half rate between the
        // hardware's 2× decimation and interpolation filters.
        for v in voices.iter_mut() {
            let voice = &mut v.voice;
            voice.pair_input[self.pair_pos] = voice.input;
            voice.pair_freeze |= voice.freeze;
        }
        if self.pair_pos == 0 {
            self.pair_pos = 1;
            for v in voices.iter_mut() {
                v.output.write(index, v.pair_output);
            }
            self.write_stereo(index, self.stereo_pair_output);
            return;
        }
        self.pair_pos = 0;

        for v in voices.iter_mut() {
            let voice = &mut v.voice;
            voice.decimator.push(voice.pair_input[0]);
            voice.decimator.push(voice.pair_input[1]);
        }
        self.begin_sample(voices, controls, width);
        Self::start_grains(voices, rng);
        Self::render(voices, true);

        let first = self.finish_sample(voices, 0, |v, x| v.output.write(index, x));
        self.write_stereo(index, first);
        self.begin_sample(voices, controls, width);
        self.stereo_pair_output = self.finish_sample(voices, 1, |v, x| v.pair_output = x);
    }
}
