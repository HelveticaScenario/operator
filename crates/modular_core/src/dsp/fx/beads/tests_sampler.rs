use super::super::tests::{SR, poly_volts, rms};
use super::super::wav_load::MIN_LOADED_FRAMES;
use super::*;
use crate::types::{OutputStruct, SampleBuffer};

const PATH: &str = "pad.wav";

fn wav_data(channels: Vec<Vec<f32>>, sample_rate: f32) -> HashMap<String, Arc<WavData>> {
    let data = WavData::new(SampleBuffer::from_samples(channels, sample_rate), None);
    HashMap::from([(PATH.to_string(), Arc::new(data))])
}

/// One second of a 220 Hz sine per channel, at `sample_rate`.
fn sine_wav(channels: usize, sample_rate: f32) -> HashMap<String, Arc<WavData>> {
    let n = sample_rate as usize;
    let ch = (0..n)
        .map(|i| (std::f32::consts::TAU * 220.0 * i as f32 / sample_rate).sin() * 0.5)
        .collect::<Vec<_>>();
    wav_data(vec![ch; channels], sample_rate)
}

fn sampler_params(quality: GranularQuality, channels: usize) -> GranularSamplerParams {
    GranularSamplerParams {
        wav: Wav::new(PATH.to_string(), channels),
        trigger: poly_volts(&[0.0]),
        time: poly_volts(&[2.5]),
        size: poly_volts(&[2.0]),
        shape: None,
        freq: None,
        root_freq: None,
        spread: None,
        width: None,
        quality,
        loaded: None,
    }
}

/// Build a module the way the engine does: `init`, `prepare_resources`,
/// then `on_patch_update`.
fn make(params: GranularSamplerParams, wav: &HashMap<String, Arc<WavData>>) -> GranularSampler {
    let mut m = construct(params, wav);
    m.on_patch_update();
    m
}

/// A module up to the point where state transfer happens.
fn construct(
    params: GranularSamplerParams,
    wav: &HashMap<String, Arc<WavData>>,
) -> GranularSampler {
    let channels = sampler_channel_count(&params);
    let mut outputs = GranularSamplerOutputs::default();
    outputs.set_all_channels(channels);
    let mut m = GranularSampler {
        outputs,
        state: Default::default(),
        params,
        channel_state: (0..channels).map(|_| Default::default()).collect(),
        _channel_count: channels,
        _block_index: Default::default(),
    };
    m.init(SR);
    m.prepare_resources_impl(wav, SR);
    m
}

/// Transfer `old`'s state into `new` as a patch update does.
fn transfer(old: &mut GranularSampler, new: &mut GranularSampler) {
    std::mem::swap(&mut old.state, &mut new.state);
    std::mem::swap(&mut old.channel_state, &mut new.channel_state);
    new.on_patch_update();
}

fn buffer(m: &GranularSampler, voice: usize) -> &GrainBuffer {
    &m.channel_state[voice].voice.granular.buffer
}

/// Trigger every voice once at sample 1000 and return each output channel
/// over the following half second.
fn play(m: &mut GranularSampler) -> Vec<Vec<f32>> {
    let voices = m.channel_state.len();
    let mut out = vec![Vec::new(); voices];
    for n in 0..1000 + 24_000 {
        let level = if (1000..1010).contains(&n) { 5.0 } else { 0.0 };
        m.params.trigger = poly_volts(&vec![level; voices]);
        m.update(SR);
        if n >= 1000 {
            for (ch, o) in out.iter_mut().enumerate() {
                o.push(m.outputs.voices.get(ch));
            }
        }
    }
    out
}

#[test]
fn buffer_length_follows_the_file_at_each_quality() {
    let wav = sine_wav(1, 44100.0);
    for (quality, frames) in [
        (GranularQuality::Digital, 48_000),
        (GranularQuality::Vintage, 32_000),
        (GranularQuality::CleanTape, 24_000),
        (GranularQuality::DirtyTape, 24_000),
    ] {
        let m = make(sampler_params(quality, 1), &wav);
        assert_eq!(buffer(&m, 0).max_delay(), frames, "{quality:?}");
        assert_eq!(buffer(&m, 0).duration(), frames, "{quality:?}");
    }
}

#[test]
fn short_files_pad_to_the_shortest_buffer() {
    let short = wav_data(vec![vec![0.5; 10]], SR);
    let m = make(sampler_params(GranularQuality::Digital, 1), &short);
    assert_eq!(buffer(&m, 0).max_delay() as usize, MIN_LOADED_FRAMES);
}

#[test]
fn grains_play_deep_into_a_long_file() {
    // A minute at 48 kHz, silent except for its first second, where grains
    // at `time` 0 begin.
    let n = 60 * SR as usize;
    let mut ch = vec![0.0; n];
    for (i, x) in ch.iter_mut().take(SR as usize).enumerate() {
        *x = (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin() * 0.5;
    }
    let wav = wav_data(vec![ch], SR);
    let mut p = sampler_params(GranularQuality::Digital, 1);
    p.time = poly_volts(&[0.0]);
    let mut m = make(p, &wav);
    assert_eq!(buffer(&m, 0).max_delay() as usize, n);
    let out = play(&mut m);
    assert!(rms(&out[0]) > 0.2, "{}", rms(&out[0]));
}

#[test]
fn voices_on_one_channel_share_its_buffer() {
    let wav = sine_wav(1, SR);
    let mut p = sampler_params(GranularQuality::Digital, 1);
    p.trigger = poly_volts(&[0.0, 0.0, 0.0]);
    let m = make(p, &wav);
    assert_eq!(m.channel_state.len(), 3);
    assert!(buffer(&m, 0).shares_with(buffer(&m, 1)));
    assert!(buffer(&m, 0).shares_with(buffer(&m, 2)));
}

#[test]
fn triggered_grains_play_the_file_in_every_quality() {
    let wav = sine_wav(1, SR);
    for quality in ALL_QUALITIES {
        let mut m = make(sampler_params(quality, 1), &wav);
        let out = play(&mut m);
        assert!(out[0].iter().all(|x| x.is_finite()), "{quality:?}");
        assert!(rms(&out[0]) > 0.2, "{quality:?}: {}", rms(&out[0]));
    }
}

#[test]
fn untriggered_voices_stay_silent() {
    let wav = sine_wav(1, SR);
    let mut m = make(sampler_params(GranularQuality::Digital, 1), &wav);
    for _ in 0..24_000 {
        m.update(SR);
        assert_eq!(m.outputs.voices.get(0), 0.0);
        assert_eq!(m.outputs.sample.get(0), 0.0);
    }
}

#[test]
fn each_voice_plays_its_own_channel_of_the_file() {
    let n = SR as usize;
    let wav = wav_data(vec![vec![0.5; n], vec![0.0; n]], SR);
    let mut m = make(sampler_params(GranularQuality::Digital, 2), &wav);
    assert_eq!(m.channel_state.len(), 2);
    let out = play(&mut m);
    assert!(rms(&out[0]) > 0.2, "left {}", rms(&out[0]));
    assert!(rms(&out[1]) < 1e-6, "right {}", rms(&out[1]));
}

#[test]
fn a_new_module_keeps_the_buffer_it_inherits_for_the_same_file() {
    let wav = sine_wav(1, SR);
    let mut old = make(sampler_params(GranularQuality::Digital, 1), &wav);
    // Playing freezes the old voice's buffer; a freshly loaded one is not.
    play(&mut old);
    let mut new = construct(sampler_params(GranularQuality::Digital, 1), &wav);
    transfer(&mut old, &mut new);
    assert!(buffer(&new, 0).frozen());
    // Later patch updates keep it too.
    new.on_patch_update();
    assert!(buffer(&new, 0).frozen());
}

#[test]
fn a_quality_change_loads_the_file_again() {
    let wav = sine_wav(1, 44100.0);
    let mut old = make(sampler_params(GranularQuality::Digital, 1), &wav);
    let mut new = construct(sampler_params(GranularQuality::DirtyTape, 1), &wav);
    transfer(&mut old, &mut new);
    assert_eq!(buffer(&new, 0).max_delay(), 24_000);
    new.on_patch_update();
    assert_eq!(buffer(&new, 0).max_delay(), 24_000);
    let out = play(&mut new);
    assert!(rms(&out[0]) > 0.2, "{}", rms(&out[0]));
}

#[test]
fn a_missing_file_plays_silence() {
    let mut m = make(sampler_params(GranularQuality::Vintage, 1), &HashMap::new());
    let out = play(&mut m);
    assert!(out[0].iter().all(|&x| x == 0.0));
}

const ALL_QUALITIES: [GranularQuality; 4] = [
    GranularQuality::Digital,
    GranularQuality::Vintage,
    GranularQuality::CleanTape,
    GranularQuality::DirtyTape,
];

#[test]
fn negative_sizes_play_grains_reversed() {
    // A rising ramp: forward grains rise, reversed grains fall.
    let n = SR as usize;
    let ramp = (0..n).map(|i| i as f32 / n as f32 * 0.5).collect();
    let wav = wav_data(vec![ramp], SR);
    let slope = |size: f32| {
        let mut p = sampler_params(GranularQuality::Digital, 1);
        p.size = poly_volts(&[size]);
        p.shape = poly_volts(&[0.0]);
        let out = play(&mut make(p, &wav));
        out[0][4000] - out[0][2000]
    };
    assert!(slope(3.0) > 0.01, "forward {}", slope(3.0));
    assert!(slope(-3.0) < -0.01, "reversed {}", slope(-3.0));
}
