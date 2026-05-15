//! Full-featured Plaits voice module based on Mutable Instruments Plaits.
//! Adapted from VCV Rack implementation with additional features.
//!
//! This module wraps the complete mi-plaits-dsp Voice struct, providing:
//! - All 24 synthesis engines with engine selection
//! - Built-in Low Pass Gate (LPG) with color and decay parameters
//! - Trigger delay (1ms) for sequencer/MIDI interface timing correction
//! - Modulation attenuverters for FM, timbre, and morph
//! - Level input for dynamics/VCA control
//! - Proper modulation routing matching the original hardware

use deserr::Deserr;
use mi_plaits_dsp::voice::{Modulations, Patch, Voice};
use schemars::JsonSchema;

use crate::{
    dsp::utils::{SchmittTrigger, voct_to_midi},
    patch::Patch as ModularPatch,
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
    types::{Clickless, Connect},
};

/// Block size for rendering - matches VCV Rack's implementation
const BLOCK_SIZE: usize = 12;

/// Internal sample rate for the Plaits engine (designed for 48kHz)
const ENGINE_SAMPLE_RATE: f32 = 48000.0;

/// Synthesis engine selection for Plaits
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum PlaitsEngine {
    /// Virtual analog oscillator with VCF - classic subtractive synthesis
    #[default]
    VaVcf,
    /// Phase distortion synthesis
    PhaseDistortion,
    /// Six-operator FM synthesis (bank A)
    SixOpA,
    /// Six-operator FM synthesis (bank B)
    SixOpB,
    /// Six-operator FM synthesis (bank C)
    SixOpC,
    /// Wave terrain synthesis
    WaveTerrain,
    /// String machine emulation
    StringMachine,
    /// Chiptune waveforms with arpeggiator
    Chiptune,
    /// Virtual analog dual oscillator
    VirtualAnalog,
    /// Waveshaping oscillator
    Waveshaping,
    /// Two-operator FM synthesis
    TwoOpFm,
    /// Granular formant oscillator
    GranularFormant,
    /// Harmonic/additive oscillator
    Additive,
    /// Wavetable oscillator
    Wavetable,
    /// Chord generator
    Chords,
    /// Vowel and speech synthesis
    Speech,
    /// Swarm oscillator
    Swarm,
    /// Filtered noise
    FilteredNoise,
    /// Particle noise
    ParticleNoise,
    /// Inharmonic string modeling
    InharmonicString,
    /// Modal resonator
    ModalResonator,
    /// Analog bass drum
    BassDrum,
    /// Analog snare drum
    SnareDrum,
    /// Analog hi-hat
    HiHat,
}

impl PlaitsEngine {
    /// Convert engine enum to index used by mi-plaits-dsp
    fn to_index(self) -> usize {
        match self {
            PlaitsEngine::VaVcf => 0,
            PlaitsEngine::PhaseDistortion => 1,
            PlaitsEngine::SixOpA => 2,
            PlaitsEngine::SixOpB => 3,
            PlaitsEngine::SixOpC => 4,
            PlaitsEngine::WaveTerrain => 5,
            PlaitsEngine::StringMachine => 6,
            PlaitsEngine::Chiptune => 7,
            PlaitsEngine::VirtualAnalog => 8,
            PlaitsEngine::Waveshaping => 9,
            PlaitsEngine::TwoOpFm => 10,
            PlaitsEngine::GranularFormant => 11,
            PlaitsEngine::Additive => 12,
            PlaitsEngine::Wavetable => 13,
            PlaitsEngine::Chords => 14,
            PlaitsEngine::Speech => 15,
            PlaitsEngine::Swarm => 16,
            PlaitsEngine::FilteredNoise => 17,
            PlaitsEngine::ParticleNoise => 18,
            PlaitsEngine::InharmonicString => 19,
            PlaitsEngine::ModalResonator => 20,
            PlaitsEngine::BassDrum => 21,
            PlaitsEngine::SnareDrum => 22,
            PlaitsEngine::HiHat => 23,
        }
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct PlaitsParams {
    /// Pitch input in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: PolySignal,

    /// Synthesis engine selection
    engine: PlaitsEngine,

    /// Harmonics parameter (-5V to +5V, bipolar, default 0V) - function varies per engine
    #[deserr(default)]
    harmonics: Option<PolySignal>,

    /// Timbre parameter (-5V to +5V, bipolar, default 0V) - function varies per engine
    #[deserr(default)]
    timbre: Option<PolySignal>,

    /// Morph parameter (-5V to +5V, bipolar, default 0V) - function varies per engine
    #[deserr(default)]
    morph: Option<PolySignal>,

    /// FM input (-5V to +5V) - frequency modulation
    #[deserr(default)]
    fm: Option<PolySignal>,

    /// Timbre CV attenuverter (-5 to 5) - scales timbre modulation
    #[deserr(default)]
    timbre_amt: Option<PolySignal>,

    /// FM CV attenuverter (-5 to 5) - scales frequency modulation
    #[deserr(default)]
    fm_amt: Option<PolySignal>,

    /// Morph CV attenuverter (-5 to 5) - scales morph modulation
    #[deserr(default)]
    morph_amt: Option<PolySignal>,

    /// Trigger input - gates/triggers the internal envelope
    #[signal(type = trig, range = (0.0, 5.0))]
    #[deserr(default)]
    trigger: Option<PolySignal>,

    /// Level/dynamics input (0-5V) - controls VCA/LPG
    #[signal(range = (0.0, 5.0))]
    #[deserr(default)]
    level: Option<PolySignal>,

    /// LPG color (0-5V) - lowpass gate filter response (low = mellow, high = bright)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    lpg_color: Option<PolySignal>,

    /// LPG decay (0-5V) - lowpass gate envelope decay time
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    lpg_decay: Option<PolySignal>,

    /// Main/Aux Mix (0-5v) - Crossfade mix between main and aux outputs at the primary output.
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    mix: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PlaitsOutputs {
    #[output("mix", "mixed synthesis output", default, range = (-5.0, 5.0))]
    mix: PolyOutput,

    #[output("main", "main synthesis output", range = (-5.0, 5.0))]
    main: PolyOutput,

    #[output("aux", "auxiliary output — varies per engine", range = (-5.0, 5.0))]
    aux: PolyOutput,
}

/// Per-channel voice state
struct PlaitsChannelState {
    voice: Voice<'static>,
    main_buffer: [f32; BLOCK_SIZE],
    aux_buffer: [f32; BLOCK_SIZE],
    // Smoothed parameters
    harmonics: Clickless,
    timbre: Clickless,
    morph: Clickless,
    lpg_color: Clickless,
    lpg_decay: Clickless,
    mix: Clickless,
    // Trigger state tracking
    /// Schmitt trigger for edge detection with hysteresis
    trigger_schmitt: SchmittTrigger,
    /// Latched trigger value - captures any trigger that occurred since last block render
    /// This ensures short triggers (even 1 sample) aren't missed between block boundaries
    trigger_latch: bool,
}

impl Default for PlaitsChannelState {
    fn default() -> Self {
        let mut voice = Voice::new(BLOCK_SIZE, ENGINE_SAMPLE_RATE);
        voice.init();
        Self {
            voice,
            main_buffer: [0.0; BLOCK_SIZE],
            aux_buffer: [0.0; BLOCK_SIZE],
            harmonics: Clickless::default(),
            timbre: Clickless::default(),
            morph: Clickless::default(),
            lpg_color: Clickless::default(),
            lpg_decay: Clickless::default(),
            mix: Clickless::default(),
            trigger_schmitt: SchmittTrigger::default(),
            trigger_latch: false,
        }
    }
}

/// Full-featured Plaits macro-oscillator with all 24 engines, LPG, and modulation routing.
///
/// For detailed engine descriptions and parameter behavior, see the
/// [Mutable Instruments Plaits documentation](https://pichenettes.github.io/mutable-instruments-documentation/modules/plaits/).
///
/// Engines (selected via `engine` param):
/// - Virtual analog VCF (classic subtractive)
/// - Phase distortion
/// - Six-op FM (3 banks)
/// - Wave terrain
/// - String machine
/// - Chiptune
/// - Virtual analog (dual oscillator)
/// - Waveshaping
/// - Two-operator FM
/// - Granular formant
/// - Harmonic/additive
/// - Wavetable
/// - Chords
/// - Vowel/speech synthesis
/// - Swarm
/// - Filtered noise
/// - Particle noise
/// - Inharmonic strings
/// - Modal resonator
/// - Analog bass drum
/// - Analog snare drum
/// - Analog hi-hat
#[module(name = "$macro", args(freq, engine), has_init)]
pub struct Plaits {
    outputs: PlaitsOutputs,
    params: PlaitsParams,
    state: PlaitsState,
}

/// State for the Plaits module.
struct PlaitsState {
    channels: Vec<PlaitsChannelState>,
    buffer_pos: usize,
    sample_rate: f32,
}

impl Default for PlaitsState {
    fn default() -> Self {
        Self {
            channels: Vec::new(),   // Will be initialized in init()
            buffer_pos: BLOCK_SIZE, // Start exhausted to trigger initial render
            sample_rate: 0.0,
        }
    }
}

impl Plaits {
    /// Initialize the module with the given sample rate.
    /// Called once at construction time by the macro-generated constructor.
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
        self.state.channels = Vec::with_capacity(PORT_MAX_CHANNELS);
        for _ in 0..PORT_MAX_CHANNELS {
            let mut voice = Voice::new(BLOCK_SIZE, sample_rate);
            voice.init();
            self.state.channels.push(PlaitsChannelState {
                voice,
                ..PlaitsChannelState::default()
            });
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        // sample_rate is fixed at construction, no need to handle changes

        let num_channels = self.channel_count().max(1);

        // Track triggers on every sample to catch short pulses
        // Triggers can be as short as 1 sample, so we need to detect rising edges
        // and latch them until the next block render.
        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];
            let trigger_val = self.params.trigger.value_or(ch, 0.0);

            // Detect rising edge using Schmitt trigger for noise immunity
            if state.trigger_schmitt.process(trigger_val) {
                // Rising edge detected - latch high trigger value
                state.trigger_latch = true;
            }
        }

        // Render when buffer is exhausted
        if self.state.buffer_pos >= BLOCK_SIZE {
            self.render_block(num_channels);
            self.state.buffer_pos = 0;
        }

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];
            // Output scaling: Plaits outputs ±1.0, scale to ±5V (inverted to match hardware)
            let main = -state.main_buffer[self.state.buffer_pos] * 5.0;
            let aux = -state.aux_buffer[self.state.buffer_pos] * 5.0;

            self.outputs.main.set(ch, main);
            self.outputs.aux.set(ch, aux);

            state
                .mix
                .update(self.params.mix.value_or(ch, 0.0).clamp(0.0, 5.0));

            let mix = state.mix / 5.0;

            self.outputs
                .mix
                .set(ch, (main * (mix - 1.0).abs()) + (aux * mix))
        }

        self.state.buffer_pos += 1;
    }

    fn render_block(&mut self, num_channels: usize) {
        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            // Update smoothed parameters
            state
                .harmonics
                .update(self.params.harmonics.value_or(ch, 0.0).clamp(-5.0, 5.0));
            state
                .timbre
                .update(self.params.timbre.value_or(ch, 0.0).clamp(-5.0, 5.0));
            state
                .morph
                .update(self.params.morph.value_or(ch, 0.0).clamp(-5.0, 5.0));
            state
                .lpg_color
                .update(self.params.lpg_color.value_or(ch, 2.5).clamp(0.0, 5.0));
            state
                .lpg_decay
                .update(self.params.lpg_decay.value_or(ch, 2.5).clamp(0.0, 5.0));

            // Get engine index from enum
            let engine_index = self.params.engine.to_index();

            let patch = Patch {
                note: voct_to_midi(self.params.freq.get_value(ch)),
                harmonics: ((*state.harmonics + 5.0) / 10.0).clamp(0.0, 1.0),
                timbre: ((*state.timbre + 5.0) / 10.0).clamp(0.0, 1.0),
                morph: ((*state.morph + 5.0) / 10.0).clamp(0.0, 1.0),
                engine: engine_index,
                decay: (*state.lpg_decay / 5.0).clamp(0.0, 1.0),
                lpg_colour: (*state.lpg_color / 5.0).clamp(0.0, 1.0),
                // Modulation amounts (attenuverters)
                frequency_modulation_amount: (self.params.fm_amt.value_or(ch, 0.0) / 5.0)
                    .clamp(-1.0, 1.0),
                timbre_modulation_amount: (self.params.timbre_amt.value_or(ch, 0.0) / 5.0)
                    .clamp(-1.0, 1.0),
                morph_modulation_amount: (self.params.morph_amt.value_or(ch, 0.0) / 5.0)
                    .clamp(-1.0, 1.0),
            };

            // Build the Modulations struct
            // FM: ±5V range, scaled to ±1.0 then multiplied by ~6 (VCV convention)
            let fm_val = self.params.fm.value_or(ch, 0.0);
            let fm_mod = fm_val / 5.0 * 6.0;

            // Use latched trigger value to ensure short triggers aren't missed
            // The latch captures any rising edge that occurred since last block render
            let trigger_mod = if state.trigger_latch { 1.0 } else { 0.0 };

            // Level: 0-5V scaled to 0-1
            let level_val = self.params.level.value_or(ch, 0.0);
            let level_mod = (level_val / 8.0).clamp(0.0, 1.0);

            // Operator merges Plaits' knob + CV jack into a single port per
            // destination, so harmonics/timbre/morph have no concept of an
            // external modulation source: the smoothed signal feeds the
            // patch.* base values, and timbreAmt / morphAmt route the
            // internal trigger envelope into those destinations.
            let modulations = Modulations {
                engine: 0.0, // No CV modulation of engine
                note: 0.0,   // No additional note modulation
                frequency: fm_mod,
                harmonics: 0.0,
                timbre: 0.0,
                morph: 0.0,
                trigger: trigger_mod,
                level: level_mod,
                frequency_patched: !self.params.fm.is_disconnected(),
                timbre_patched: false,
                morph_patched: false,
                trigger_patched: !self.params.trigger.is_disconnected(),
                level_patched: !self.params.level.is_disconnected(),
            };

            // Render the voice
            state.voice.render(
                &patch,
                &modulations,
                &mut state.main_buffer,
                &mut state.aux_buffer,
            );

            // Clear trigger latch and max after rendering
            // This ensures the trigger is only processed once per event
            state.trigger_latch = false;
        }
    }
}

message_handlers!(impl Plaits {});
