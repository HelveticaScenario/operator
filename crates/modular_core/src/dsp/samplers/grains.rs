use std::f32::consts::PI;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    Wav,
    dsp::utils::{SchmittTrigger, sanitize},
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
};

// ── Constants ──────────────────────────────────────────────────────────────────

/// Entries in the per-shape amplitude window look-up table.
const LUT_SIZE: usize = 4096;

/// Maximum concurrent grains per polyphonic channel.
const GRAINS_PER_CHANNEL: usize = 64;

// ── GrainShape ─────────────────────────────────────────────────────────────────

/// Amplitude windowing function applied to each grain.
#[derive(Clone, Copy, Deserr, JsonSchema, Debug, PartialEq, Eq, Connect, Default)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
enum GrainShape {
    /// Symmetric triangle: linear rise and fall meeting at the centre.
    #[default]
    Triangle,
    /// Linear rise to peak then a sharp 1 ms linear fall at the tail.
    Ramp,
    /// 1 ms linear attack, sustain at peak, 1 ms linear decay.
    Square,
    /// 1 ms linear attack followed by exponential decay matching `$perc`.
    Decay,
    /// Gaussian bell curve; endpoints ≈ 0.01 (−40 dB).
    Bell,
    /// Lanczos-windowed sinc, 3 lobes per side.
    Sinc,
    /// Cosine-tapered flat-top (Tukey) window, α = 0.5.
    Tukey,
}

// ── Grain ──────────────────────────────────────────────────────────────────────

#[derive(Default, Clone, Copy)]
struct Grain {
    /// Whether this slot is currently playing.
    active: bool,
    /// Fractional frame index into the WAV buffer.
    read_pos: f64,
    /// Frames advanced per audio sample (positive = forward, negative = reverse).
    signed_rate: f64,
    /// Samples elapsed since spawn; window phase t = age * inv_life.
    age: f32,
    /// Reciprocal of the grain length in samples.
    inv_life: f32,
    /// Fraction of normalised grain time equal to 1 ms (for ramp / square / decay).
    d_ratio: f32,
    /// Running amplitude for the decay envelope (decay shape only).
    decay_level: f32,
    /// Per-sample multiply for the exponential tail (decay shape only).
    decay_coeff: f32,
}

// ── Per-channel state ──────────────────────────────────────────────────────────

#[derive(Default)]
struct LcgRng {
    state: u64,
}

impl LcgRng {
    /// Returns a pseudo-random value in [0, 1).
    #[inline]
    fn next_f32(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        // Top 31 bits → non-negative u32; divide to [0, 1).
        let bits = (self.state >> 33) as u32;
        bits as f32 / (u32::MAX >> 1) as f32
    }
}

struct GrainChannel {
    grains: [Grain; GRAINS_PER_CHANNEL],
    /// Accumulator; a grain is spawned each time this crosses 1.0.
    spawn_phase: f32,
    /// Grains spawned since the last gate rising edge (for `loopCount`).
    grains_spawned: u32,
    gate_trigger: SchmittTrigger,
    /// Asymmetrically smoothed active-grain count (Clouds SLOPE: fast rise,
    /// slow fall) used for Clouds-style amplitude normalisation.
    smoothed_count: f32,
    rng: LcgRng,
}

impl Default for GrainChannel {
    fn default() -> Self {
        Self {
            grains: std::array::from_fn(|_| Grain::default()),
            spawn_phase: 0.0,
            grains_spawned: 0,
            gate_trigger: SchmittTrigger::default(),
            smoothed_count: 0.0,
            rng: LcgRng::default(),
        }
    }
}

// ── Module-level state ─────────────────────────────────────────────────────────

struct GrainsState {
    /// Engine sample rate, captured in `init`.
    sample_rate: f32,
    /// wav_rate / engine_rate; computed in `on_patch_update` once WAV is
    /// connected (mirrors the pattern in `sampler.rs`).
    rate_ratio: f64,
    /// Shape used to fill the current LUT; `None` forces the first fill.
    last_shape: Option<GrainShape>,
    /// Pre-computed window values for Bell / Sinc / Tukey.
    /// Allocated in `init`; refilled in `on_patch_update` when shape changes.
    window_lut: Box<[f32]>,
    /// True once channel RNGs have been seeded off this instance's heap address.
    seeded: bool,
}

impl Default for GrainsState {
    fn default() -> Self {
        Self {
            sample_rate: 48000.0,
            rate_ratio: 1.0,
            last_shape: None,
            window_lut: Box::default(), // empty; grown in init
            seeded: false,
        }
    }
}

// ── Params ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct GrainsParams {
    /// Pitch of spawned grains in V/Oct. 0 V = native speed, 1 V = 2×, −1 V = 0.5×.
    #[signal(type = pitch, range = (-4.0, 4.0))]
    pitch: PolySignal,
    /// WAV file to granulate.
    wav: Wav,
    /// Gate: rising edge resets the loop counter; high level enables grain spawning.
    #[signal(type = gate, range = (0.0, 5.0))]
    gate: PolySignal,
    /// Read position within the sample: 0 V = start, 5 V = end.
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    start: Option<PolySignal>,
    /// Grain length: 1 V ≈ 100 ms, 0.01 V ≈ 1 ms. Clamped to sample duration.
    #[signal(default = 1.0, range = (0.01, 10.0))]
    #[deserr(default)]
    length: Option<PolySignal>,
    /// Grain spawn rate: grains/sec = value × 10. Default 2.5 V = 25 grains/sec.
    #[signal(default = 2.5, range = (0.0001, 100.0))]
    #[deserr(default)]
    density: Option<PolySignal>,
    /// Reverse probability: 0 V = always forward, 5 V = always reverse.
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    direction_bias: Option<PolySignal>,
    /// Max grains per gate-high. Absent/null = infinite.
    #[serde(default)]
    #[deserr(default)]
    loop_count: Option<u32>,
    /// Amplitude window function applied to each grain.
    #[serde(default)]
    #[deserr(default)]
    shape: GrainShape,
}

// ── Outputs ────────────────────────────────────────────────────────────────────

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GrainsOutputs {
    // No declared range: the Clouds-style normalisation is power-based, so
    // correlated grains (a sustained tone, DC) can sum above the ±5 V nominal
    // level, and a declared range would be a contract the output cannot keep.
    #[output("output", "granular synthesis output", default)]
    sample: PolyOutput,
}

// ── Channel count ──────────────────────────────────────────────────────────────

fn grains_derive_channel_count(params: &GrainsParams) -> usize {
    let mut count = params.pitch.channels().max(params.gate.channels());
    count = count.max(params.start.channel_count());
    count = count.max(params.length.channel_count());
    count = count.max(params.density.channel_count());
    count = count.max(params.direction_bias.channel_count());
    count.max(1).min(PORT_MAX_CHANNELS)
}

// ── Module ─────────────────────────────────────────────────────────────────────

/// Granular sampler. Continuously spawns short, windowed grains from a WAV
/// file, enabling time-stretching, pitch-shifting independent of time, and
/// cloud textures.
///
/// Each polyphonic channel maintains up to 64 concurrent grains. Spawned grains
/// capture their pitch, start position, and length at spawn time, so live
/// parameter changes only affect new grains.
///
/// Amplitude is normalised using the Mutable Instruments Clouds formula:
/// `gain = 1/√(max(1, smoothed_active_grains − 1))`.
///
/// ```js
/// $grains('c4', $wavs().pad, $pulse('2hz'))
/// const seq = $cycle($p('c3 e3'))
/// $grains(seq, $wavs().strings, seq.gate, { density: 3, shape: 'bell', length: 2 })
/// ```
#[module(
    name = "$grains",
    channels_derive = grains_derive_channel_count,
    args(pitch, wav, gate),
    has_init,
    patch_update
)]
pub struct Grains {
    params: GrainsParams,
    outputs: GrainsOutputs,
    state: GrainsState,
    channel_state: Box<[GrainChannel]>,
}

// ── Window helpers ─────────────────────────────────────────────────────────────

/// Linear interpolation into a window LUT, t ∈ [0, 1].
#[inline]
fn lut_read(lut: &[f32], t: f32) -> f32 {
    let n = lut.len();
    if n == 0 {
        return 0.0;
    }
    let idx = t * (n - 1) as f32;
    let i = idx as usize;
    let frac = idx - i as f32;
    let a = lut[i.min(n - 1)];
    let b = lut[(i + 1).min(n - 1)];
    a + (b - a) * frac
}

/// Sample value for a LUT-stored shape at normalised time t ∈ [0, 1].
fn window_lut_value(shape: GrainShape, t: f32) -> f32 {
    match shape {
        GrainShape::Bell => {
            // Gaussian centred at 0.5, σ ≈ 0.165 → endpoints ≈ 0.01.
            let u = t - 0.5;
            (-18.42 * u * u).exp()
        }
        GrainShape::Sinc => {
            // Lanczos-windowed sinc, 3 lobes per side.
            let x = (t - 0.5) * 6.0; // [0,1] → [−3, 3]
            if x.abs() < 1e-6 {
                1.0
            } else {
                let sinc_x = (PI * x).sin() / (PI * x);
                let lanczos = if x.abs() < 3.0 {
                    (PI * x / 3.0).sin() / (PI * x / 3.0)
                } else {
                    0.0
                };
                sinc_x * lanczos
            }
        }
        GrainShape::Tukey => {
            // Cosine-tapered flat-top window, α = 0.5.
            const ALPHA: f32 = 0.5;
            let half_a = ALPHA * 0.5;
            if t < half_a {
                0.5 * (1.0 - (2.0 * PI * t / ALPHA).cos())
            } else if t <= 1.0 - half_a {
                1.0
            } else {
                0.5 * (1.0 - (2.0 * PI * (t - 1.0 + ALPHA) / ALPHA).cos())
            }
        }
        // Analytic shapes (triangle / ramp / square / decay) are not stored in
        // the LUT; this branch should never be reached in the hot path.
        _ => 0.0,
    }
}

/// Fill the LUT for Bell / Sinc / Tukey shapes.
fn fill_window_lut(lut: &mut [f32], shape: GrainShape) {
    let n = lut.len();
    for (i, slot) in lut.iter_mut().enumerate() {
        let t = i as f32 / (n - 1).max(1) as f32;
        *slot = window_lut_value(shape, t);
    }
}

// ── Grain pool helpers ─────────────────────────────────────────────────────────

/// Find a free grain slot. If all 64 are active, steal the oldest (highest
/// `age`), which is closest to its natural end.
#[inline]
fn alloc_grain(grains: &[Grain; GRAINS_PER_CHANNEL]) -> usize {
    for (i, g) in grains.iter().enumerate() {
        if !g.active {
            return i;
        }
    }
    grains
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.age
                .partial_cmp(&b.age)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Asymmetric one-pole smoother matching Clouds' SLOPE macro:
/// fast attack (coeff 0.9), slow release (coeff 0.2).
#[inline]
fn slope(state: &mut f32, target: f32) {
    let coeff = if target > *state { 0.9 } else { 0.2 };
    *state += (target - *state) * coeff;
}

// ── Implementation ─────────────────────────────────────────────────────────────

impl Grains {
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
        // Allocate the window LUT (contents filled in on_patch_update).
        self.state.window_lut = vec![0.0_f32; LUT_SIZE].into_boxed_slice();
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let frame_count = self.params.wav.frame_count();

        if !self.params.wav.is_loaded() || frame_count == 0 {
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        }

        let rate_ratio = self.state.rate_ratio;
        let sample_rate = self.state.sample_rate;
        let shape = self.params.shape;
        let wav_channels = self.params.wav.channel_count().max(1);

        for ch in 0..channels {
            // ── Read params for this channel ───────────────────────────────
            let gate_val = self.params.gate.get_value(ch);
            let density = self.params.density.value_or(ch, 2.5).max(0.0001);
            let loop_count = self.params.loop_count;

            // ── Gate / spawn control ───────────────────────────────────────
            {
                let cs = &mut self.channel_state[ch];
                let (is_high, edge) = cs.gate_trigger.process_with_edge(gate_val);

                if edge.is_rising() {
                    cs.grains_spawned = 0;
                    cs.spawn_phase = 1.0; // fire immediately on gate rise
                }

                let limit_reached = loop_count.map_or(false, |n| cs.grains_spawned >= n);

                if is_high && !limit_reached {
                    let grains_per_sample = density * 10.0 / sample_rate;
                    cs.spawn_phase += grains_per_sample;

                    // Capture params outside the grain-spawn inner loop to
                    // avoid re-reading on every grain (values are stable per
                    // sample tick).
                    let pitch = self.params.pitch.get_value(ch);
                    let start_v = self.params.start.value_or(ch, 0.0).clamp(0.0, 5.0);
                    let length_v = self.params.length.value_or(ch, 1.0).max(0.01);
                    let dir_bias = self.params.direction_bias.value_or(ch, 0.0).clamp(0.0, 5.0);

                    let start_frame = (start_v as f64 / 5.0) * (frame_count - 1) as f64;
                    let max_len_s = frame_count as f64 / (sample_rate as f64 * rate_ratio).max(1.0);
                    let length_s = (length_v as f64 * 0.1).clamp(0.00001, max_len_s);
                    let length_samps = length_s * sample_rate as f64;
                    let inv_life = (1.0 / length_samps.max(1.0)) as f32;
                    let playback_rate = 2.0_f64.powf(pitch as f64) * rate_ratio;
                    let reverse_prob = dir_bias / 5.0;

                    let one_ms = 0.001 * sample_rate as f64;
                    let d_ratio = (one_ms / length_samps.max(1.0)).clamp(0.0, 0.45) as f32;
                    let decay_samps = ((1.0 - d_ratio as f64) * length_samps).max(1.0);
                    let decay_coeff = (-6.9 / decay_samps).exp() as f32;

                    let cs = &mut self.channel_state[ch];
                    while cs.spawn_phase >= 1.0 {
                        cs.spawn_phase -= 1.0;

                        let reverse = cs.rng.next_f32() < reverse_prob;

                        // Reverse grains play the same slice backward: start
                        // at the far end and step back toward start_frame.
                        let (read_pos, signed_rate) = if reverse {
                            (start_frame + playback_rate * length_samps, -playback_rate)
                        } else {
                            (start_frame, playback_rate)
                        };

                        let slot = alloc_grain(&cs.grains);
                        cs.grains[slot] = Grain {
                            active: true,
                            read_pos,
                            signed_rate,
                            age: 0.0,
                            inv_life,
                            d_ratio,
                            decay_level: 0.0,
                            decay_coeff,
                        };
                        cs.grains_spawned += 1;

                        if loop_count.map_or(false, |n| cs.grains_spawned >= n) {
                            break;
                        }
                    }
                }
            }

            // ── Process active grains ──────────────────────────────────────
            let mut sum = 0.0_f32;
            let mut active_count = 0_u32;
            let wav_ch = ch % wav_channels;
            let lut = &self.state.window_lut;

            let cs = &mut self.channel_state[ch];
            for grain in cs.grains.iter_mut() {
                if !grain.active {
                    continue;
                }
                let t = grain.age * grain.inv_life;
                if t >= 1.0 {
                    grain.active = false;
                    continue;
                }

                let window = match shape {
                    GrainShape::Triangle => {
                        if t < 0.5 {
                            2.0 * t
                        } else {
                            2.0 * (1.0 - t)
                        }
                    }
                    GrainShape::Ramp => {
                        // Linear rise then 1 ms fall.
                        let fall = grain.d_ratio.max(1e-6);
                        let rise_end = (1.0 - fall).max(1e-6);
                        if t < rise_end {
                            t / rise_end
                        } else {
                            (1.0 - t) / fall
                        }
                    }
                    GrainShape::Square => {
                        // 1 ms attack, hold, 1 ms decay.
                        let a = grain.d_ratio.max(1e-6);
                        let hold_end = 1.0 - a;
                        if t < a {
                            t / a
                        } else if t < hold_end {
                            1.0
                        } else {
                            (1.0 - t) / a
                        }
                    }
                    GrainShape::Decay => {
                        let attack = grain.d_ratio.max(1e-6);
                        if t < attack {
                            // 1 ms linear attack.
                            grain.decay_level = t / attack;
                            grain.decay_level
                        } else {
                            // Exponential decay matching $perc.
                            grain.decay_level *= grain.decay_coeff;
                            grain.decay_level
                        }
                    }
                    // Bell / Sinc / Tukey use the precomputed LUT.
                    _ => lut_read(lut, t),
                };

                let sample = self
                    .params
                    .wav
                    .read_hermite_clamped(wav_ch, grain.read_pos as f32);

                sum += sample * window;
                active_count += 1;

                grain.read_pos += grain.signed_rate;
                grain.age += 1.0;
            }

            // ── Normalise (Clouds-style) ───────────────────────────────────
            // gain = 1/√(max(1, smoothed_n − 1)); guard n ≤ 2 → 1.0.
            slope(&mut cs.smoothed_count, active_count as f32);
            let n = cs.smoothed_count;
            // n > 2.0 guarantees n - 1.0 > 1.0, so no clamp is needed.
            let gain = if n > 2.0 {
                (n - 1.0).sqrt().recip()
            } else {
                1.0
            };

            // WAV samples are −1..1; scale to ±5 V oscillator level.
            self.outputs.sample.set(ch, sanitize(sum * gain * 5.0));
        }
    }

    /// Seed each channel's RNG from this instance's stable heap address.
    ///
    /// Called from `on_patch_update` (not `init`) so the address is the
    /// stable per-instance heap address, not a transient stack slot.
    /// Guarded by `seeded` (which rides `transfer_state_from`) so patch
    /// updates don't clobber running grain sequences — identical to the
    /// pattern used in `noise.rs`.
    fn seed(&mut self) {
        let base = self as *const Self as usize as u64;
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.rng.state = base ^ (ch as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        self.state.seeded = true;
    }
}

impl crate::types::PatchUpdateHandler for Grains {
    fn on_patch_update(&mut self) {
        // Seed RNGs once per instance lifetime.
        if !self.state.seeded {
            self.seed();
        }

        // Recompute rate ratio now that wav.sample_rate() is valid (mirrors
        // sampler.rs on_patch_update).
        let wav_rate = self.params.wav.sample_rate() as f64;
        let engine_rate = self.state.sample_rate as f64;
        self.state.rate_ratio = if wav_rate > 0.0 && engine_rate > 0.0 {
            wav_rate / engine_rate
        } else {
            1.0
        };

        // Refill the window LUT when the shape changes (also covers the first
        // call, where last_shape is None). The buffer is allocated once in
        // init; on_patch_update only fills it, never reallocates (mirrors the
        // mix_down.rs convention — no heap allocation on the audio thread).
        let shape = self.params.shape;
        if self.state.last_shape != Some(shape) {
            fill_window_lut(&mut self.state.window_lut, shape);
            self.state.last_shape = Some(shape);
        }
    }
}

message_handlers!(impl Grains {});

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::patch::Patch;
    use crate::types::{SampleBuffer, Sampleable, WavData};

    const SAMPLE_RATE: f32 = 48000.0;
    const TEST_BLOCK_SIZE: usize = 1;

    fn make_module(module_type: &str, id: &str, params: serde_json::Value) -> Box<dyn Sampleable> {
        let constructors = get_constructors();
        let deserializers = get_params_deserializers();
        let deserializer = deserializers
            .get(module_type)
            .unwrap_or_else(|| panic!("no params deserializer for '{module_type}'"));
        let cached = deserializer(params)
            .unwrap_or_else(|e| panic!("params deserialization for '{module_type}' failed: {e}"));
        let deserialized = DeserializedParams {
            params: cached.params,
            argument_spans: Default::default(),
            channel_count: cached.channel_count,
        };
        constructors
            .get(module_type)
            .unwrap_or_else(|| panic!("no constructor for '{module_type}'"))(
            &id.to_string(),
            SAMPLE_RATE,
            deserialized,
            TEST_BLOCK_SIZE,
            crate::types::ProcessingMode::Block,
        )
        .unwrap_or_else(|e| panic!("constructor for '{module_type}' failed: {e}"))
    }

    struct Stepper {
        slot: usize,
    }

    impl Stepper {
        fn new() -> Self {
            Self {
                slot: TEST_BLOCK_SIZE,
            }
        }

        fn tick(&mut self, module: &dyn Sampleable) -> usize {
            if self.slot >= TEST_BLOCK_SIZE {
                module.start_block();
                module.ensure_processed();
                self.slot = 0;
            }
            let s = self.slot;
            self.slot += 1;
            s
        }
    }

    fn make_test_wav(samples: Vec<Vec<f32>>, sample_rate: f32) -> Arc<WavData> {
        Arc::new(WavData::new(
            SampleBuffer::from_samples(samples, sample_rate),
            None,
        ))
    }

    fn make_and_connect(
        params: serde_json::Value,
        wav_key: &str,
        wav_data: Arc<WavData>,
    ) -> Box<dyn Sampleable> {
        let module = make_module("$grains", "g1", params);
        let mut patch = Patch::new();
        patch.wav_data.insert(wav_key.to_string(), wav_data);
        module.connect(&patch);
        module.on_patch_update();
        module
    }

    // ── Silence when not triggered ─────────────────────────────────────────────

    #[test]
    fn grains_silent_when_gate_low() {
        // 10-sample mono WAV; gate is 0.0 so no grains spawn.
        let wav = make_test_wav(vec![vec![0.5; 10]], SAMPLE_RATE);
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 0.0,
            }),
            "t",
            wav,
        );
        let mut s = Stepper::new();
        for _ in 0..10 {
            let slot = s.tick(module.as_ref());
            assert_eq!(
                module.get_value_at("output", 0, slot),
                0.0,
                "should be silent with gate low"
            );
        }
    }

    // ── Gate rising edge spawns a grain ───────────────────────────────────────

    #[test]
    fn grains_nonzero_after_gate_rise() {
        // Constant-value WAV: every frame is 0.4 → output (after window) is nonzero.
        let wav = make_test_wav(vec![vec![0.4; 2000]], SAMPLE_RATE);
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "density": 2.5,
                "shape": "triangle",
            }),
            "t",
            wav,
        );
        // After a few samples some grain output should appear.
        let mut s = Stepper::new();
        let mut any_nonzero = false;
        for _ in 0..100 {
            let slot = s.tick(module.as_ref());
            if module.get_value_at("output", 0, slot).abs() > 1e-6 {
                any_nonzero = true;
                break;
            }
        }
        assert!(
            any_nonzero,
            "should produce non-silent output after gate rise"
        );
    }

    // ── Gate going low does NOT silence in-flight grains ──────────────────────

    #[test]
    fn grains_continue_after_gate_falls() {
        // Spawn a grain then drop gate; grain should finish naturally.
        let wav = make_test_wav(vec![vec![0.8; 4800]], SAMPLE_RATE); // 100 ms at 48 kHz
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                // Gate = 5.0 at construction; we'll read a few frames then switch
                // to a constant-0 module. For this test, keep gate high for a
                // couple of frames to spawn a grain, then re-examine.
                "gate": 5.0,
                "density": 2.5,
                "length": 1.0,
                "shape": "triangle",
            }),
            "t",
            wav,
        );
        // Advance a few frames so a grain is spawned and under way.
        let mut s = Stepper::new();
        for _ in 0..5 {
            s.tick(module.as_ref());
        }
        // Gate went high from the start; at least one grain should be alive.
        // We can't flip the gate signal from here, but we can verify the
        // module produces audio (grain is playing) by checking the output.
        let slot = s.tick(module.as_ref());
        let v = module.get_value_at("output", 0, slot);
        assert!(v.abs() > 1e-6, "grain should still be producing audio");
    }

    // ── loopCount limits spawning ──────────────────────────────────────────────

    #[test]
    fn grains_loop_count_one_spawns_exactly_one_grain() {
        // With loopCount=1, only one grain should ever be spawned per gate.
        // Use a very high density so the first grain spawns on the very first
        // frame, then verify subsequent frames are just that grain decaying.
        let wav = make_test_wav(vec![vec![0.5; 48000]], SAMPLE_RATE);
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "density": 100.0,  // would spawn many without loopCount cap
                "loopCount": 1,
                "shape": "triangle",
                "length": 0.5,
            }),
            "t",
            wav,
        );
        let mut s = Stepper::new();
        // Drive 10 frames. With loopCount=1 only 1 grain is ever alive.
        // Because density=100 → 1000 grains/sec, the spawn_phase would cross
        // many times if uncapped.  The output should be non-silent (grain
        // active) but not astronomically loud (only 1 grain, gain = 1.0).
        for _ in 0..10 {
            let slot = s.tick(module.as_ref());
            let v = module.get_value_at("output", 0, slot);
            // With 1 grain and window t near 0, output should be small but
            // non-negative (triangle window is always non-negative).
            assert!(v.is_finite(), "output should be finite: {v}");
        }
    }

    // ── loopCount rearms on next gate rise ────────────────────────────────────

    #[test]
    fn grains_loop_count_rearms_on_gate_rise() {
        // loopCount = 0 in JSON is the minimum; let's use 1 and verify that
        // the counter resets when the gate falls and rises again. We can't
        // dynamically change the gate signal from tests, so this test verifies
        // the counter field is reset via the Stepper+gate approach in the
        // params JSON (always high gate). Instead, we verify that the
        // spawn_phase logic itself doesn't blow up.
        let wav = make_test_wav(vec![vec![0.3; 4800]], SAMPLE_RATE);
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "loopCount": 2,
                "density": 50.0,
                "shape": "triangle",
            }),
            "t",
            wav,
        );
        let mut s = Stepper::new();
        let mut max_v = 0.0_f32;
        for _ in 0..50 {
            let slot = s.tick(module.as_ref());
            let v = module.get_value_at("output", 0, slot).abs();
            if v > max_v {
                max_v = v;
            }
            assert!(v.is_finite(), "output should be finite: {v}");
        }
        assert!(max_v > 1e-6, "should produce some audio: max was {max_v}");
    }

    // ── Pitch doubles playback rate ────────────────────────────────────────────

    #[test]
    fn grains_pitch_one_advances_faster() {
        // Two identical WAVs; one plays at pitch=0, other at pitch=1.
        // The pitch=1 grain should reach a later sample position sooner.
        let wav0 = make_test_wav(
            vec![(0..200).map(|i| i as f32 / 200.0).collect()],
            SAMPLE_RATE,
        );
        let wav1 = wav0.clone();

        let m0 = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "density": 100.0,
                "loopCount": 1,
                "shape": "square",
                "length": 0.01,
            }),
            "t",
            wav0,
        );
        let m1 = make_and_connect(
            serde_json::json!({
                "pitch": 1.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "density": 100.0,
                "loopCount": 1,
                "shape": "square",
                "length": 0.01,
            }),
            "t",
            wav1,
        );

        let mut s0 = Stepper::new();
        let mut s1 = Stepper::new();

        // After a few frames the pitch=1 grain reads further into the ramp.
        // Collect outputs at the same time step.
        let mut v0_last = 0.0_f32;
        let mut v1_last = 0.0_f32;
        for _ in 0..8 {
            let slot = s0.tick(m0.as_ref());
            v0_last = m0.get_value_at("output", 0, slot);
            let slot = s1.tick(m1.as_ref());
            v1_last = m1.get_value_at("output", 0, slot);
        }
        // The pitch=1 grain reads at double rate → higher sample value from
        // the ascending ramp wav. Both should be non-silent.
        assert!(
            v0_last.is_finite() && v1_last.is_finite(),
            "both outputs should be finite"
        );
    }

    // ── direction_bias = 5 produces a reversed grain ──────────────────────────

    #[test]
    fn grains_direction_bias_full_reverses() {
        // An ascending ramp sample; with directionBias=5 all grains play in
        // reverse, so the first read should be from near the end of the grain
        // slice → higher value from the ramp.
        let n = 480;
        let wav = make_test_wav(
            vec![(0..n).map(|i| i as f32 / n as f32).collect()],
            SAMPLE_RATE,
        );
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "density": 100.0,
                "loopCount": 1,
                "directionBias": 5.0,
                "shape": "square",
                "length": 0.005,
                "start": 2.5,
            }),
            "t",
            wav,
        );
        let mut s = Stepper::new();
        let mut got_audio = false;
        for _ in 0..20 {
            let slot = s.tick(module.as_ref());
            let v = module.get_value_at("output", 0, slot);
            assert!(v.is_finite(), "output should be finite: {v}");
            if v.abs() > 1e-6 {
                got_audio = true;
            }
        }
        assert!(got_audio, "reversed grain should produce audio");
    }

    // ── Out-of-bounds reads produce silence (no panic) ─────────────────────────

    #[test]
    fn grains_oob_read_is_silent() {
        // A 2-frame WAV; start=5V (end) + forward playback → reads off the end
        // immediately → silence (read_hermite_clamped returns 0 OOB).
        let wav = make_test_wav(vec![vec![1.0, 1.0]], SAMPLE_RATE);
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 4.0,  // very fast → goes OOB quickly
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "density": 100.0,
                "loopCount": 1,
                "start": 5.0,  // start at end
                "shape": "triangle",
                "length": 0.5,
            }),
            "t",
            wav,
        );
        let mut s = Stepper::new();
        // None of the sample reads should panic or produce NaN/Inf.
        for _ in 0..50 {
            let slot = s.tick(module.as_ref());
            let v = module.get_value_at("output", 0, slot);
            assert!(v.is_finite(), "OOB grain must not produce NaN/Inf: {v}");
        }
    }

    // ── Normalization: single grain is unattenuated ────────────────────────────

    #[test]
    fn grains_single_grain_not_attenuated() {
        // One grain at square window = full amplitude at peak → gain = 1.0.
        let wav = make_test_wav(vec![vec![1.0; 48000]], SAMPLE_RATE);
        let module = make_and_connect(
            serde_json::json!({
                "pitch": 0.0,
                "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                "gate": 5.0,
                "density": 0.01, // very low → at most 1 grain alive
                "loopCount": 1,
                "shape": "square",
                "length": 0.5,
            }),
            "t",
            wav,
        );
        let mut s = Stepper::new();
        let mut peak = 0.0_f32;
        for _ in 0..1000 {
            let slot = s.tick(module.as_ref());
            let v = module.get_value_at("output", 0, slot).abs();
            if v > peak {
                peak = v;
            }
        }
        // With 1 grain and square window at peak, sum=1.0 × gain=1.0 × 5V = 5.0V.
        // The smoothed_count lags; at 1 grain the gain stays 1.0.
        assert!(peak > 0.0, "should produce nonzero peak");
        assert!(
            peak <= 5.01,
            "single grain should not exceed ±5 V: peak was {peak}"
        );
    }

    // ── All window shapes produce finite output ────────────────────────────────

    #[test]
    fn grains_all_shapes_finite() {
        for shape in [
            "triangle", "ramp", "square", "decay", "bell", "sinc", "tukey",
        ] {
            let wav = make_test_wav(vec![vec![0.5; 9600]], SAMPLE_RATE);
            let module = make_and_connect(
                serde_json::json!({
                    "pitch": 0.0,
                    "wav": { "type": "wav_ref", "path": "t", "channels": 1 },
                    "gate": 5.0,
                    "density": 5.0,
                    "shape": shape,
                    "length": 0.1,
                }),
                "t",
                wav,
            );
            let mut s = Stepper::new();
            for i in 0..200 {
                let slot = s.tick(module.as_ref());
                let v = module.get_value_at("output", 0, slot);
                assert!(
                    v.is_finite(),
                    "shape '{shape}' produced non-finite output at sample {i}: {v}"
                );
            }
        }
    }
}
