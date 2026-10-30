# Plaits engines as native Operator modules

Goal: retire `$macro` and the `mi-plaits-dsp` crate. Port the Plaits engines that Operator cannot already build from existing modules as individual, idiomatic modules under `$mi.*`, ported directly from the C++ (`pichenettes/eurorack`, `plaits/dsp` + `stmlib/dsp`, MIT). This is not a `$macro` API replacement.

## 1. LPG audit

### `$lpg` (`crates/modular_core/src/dsp/filters/lpg.rs`)

A per-sample re-derivation of `LPGEnvelope` + `LowPassGate`, checked line by line against `plaits/dsp/envelope.h` and `plaits/dsp/fx/low_pass_gate.h`, and numerically against the compiled C++ envelope. Ping mode, time to −40 dB:

| decay | color | Plaits C++ | `$lpg`   |
| ----- | ----- | ---------- | -------- |
| 0.00  | 0.0   | 0.2023 s   | 0.2102 s |
| 0.00  | 1.0   | 0.1060 s   | 0.1104 s |
| 0.10  | 0.5   | 0.2233 s   | 0.2243 s |
| 0.50  | 0.0   | 1.6820 s   | 1.6829 s |
| 1.00  | 1.0   | 7.2800 s   | 7.2809 s |

Correct: rise coefficient (exact per-sample equivalent of 0.6/block), decay/tail coefficients, the vactrol nonlinearity (`v⁴` and `tail` from the pre-update state, gain from the post-update state, as in C++), `hf_bleed`, cutoff curve renormalised from 48 kHz, SVF Q 0.4, compressed level curve, bypass when neither input is patched.

Deviations:

- **`decay.max(0.1)` is not in Plaits.** Inherited from the Rust port. Only affects decay < 0.5 V: shortest pluck is ~4% longer. Remove it.
- Ping attack uses a fixed C4 rate (Plaits uses the voice's note) — intentional, `$lpg` has no pitch input.
- Trigger and level combine (ping from the level floor); Plaits ignores triggers for the LPG once level is patched — intentional extension, documented.
- No 1 ms trigger delay — that compensates external CV/gate skew; correct to omit.
- Uses `mi_plaits_dsp::utils::filter::Svf` — moves to the in-tree port of `stmlib::Svf` (§6) before the crate is removed.

### `$macro`'s internal LPG

Matches the C++ except: the same decay clamp; trigger delay 8 blocks instead of 5; chiptune envelope `.max(0.05)`; `level` scaled 1/8 (5 V opens the vactrol only to 0.88, unlike `$lpg`'s 1/5). The voice also runs at the host rate while VCV runs Plaits at 48 kHz. Moot once `$macro` is removed.

## 2. Engine evaluation

| #   | Engine            | Verdict        | Module(s) / existing equivalent                                                                                                                                                  |
| --- | ----------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0   | VA + VCF          | Replicable     | `$saw`/`$pulse` + sub `$pulse` an octave down → `$multimode`/`$lpf`/`$jup6f`                                                                                                     |
| 1   | Phase distortion  | **Port**       | `$mi.phaseDistortion`                                                                                                                                                            |
| 2–4 | Six-op FM         | **Port**       | `$mi.sixOp`                                                                                                                                                                      |
| 5   | Wave terrain      | **Port**       | `$mi.waveTerrain`                                                                                                                                                                |
| 6   | String machine    | Replicable after ports | `$chord` → `$mi.divideDown` → `$lpf` → `$mi.chorus`                                                                                                                     |
| 7   | Chiptune          | Replicable     | `$chord` + `$pulse` (sync for the "super square"), arpeggios via `$cycle($p.s(...))`, `$perc` envelope, triangle bass via `$saw` shape 2.5 + `$math` rounding to 16 steps        |
| 8   | Virtual analog    | Replicable     | `$saw`/`$pulse` with `sync`, detune by pitch offset                                                                                                                              |
| 9   | Waveshaping       | **Port**       | `$mi.waveshaper`                                                                                                                                                                 |
| 10  | Two-op FM         | **Port**       | `$mi.twoOp`                                                                                                                                                                      |
| 11  | Granular formant  | **Port**       | `$mi.formant` + `$mi.zOsc`                                                                                                                                                       |
| 12  | Additive          | **Port**       | `$mi.additive`                                                                                                                                                                   |
| 13  | Wavetable         | Not ported     | `$wavetable` covers it                                                                                                                                                           |
| 14  | Chords            | Replicable after ports | `$chord` with the new `inversion` (§5) → `$mi.divideDown`                                                                                                               |
| 15  | Speech            | **Port**       | `$mi.vowel` + `$mi.speak`                                                                                                                                                        |
| 16  | Swarm             | **Port**       | `$mi.swarm`                                                                                                                                                                      |
| 17  | Filtered noise    | Replicable     | `$noise` (clocked `rate`) → `$multimode`/`$bpf`                                                                                                                                  |
| 18  | Particle noise    | **Port**       | `$mi.particles`                                                                                                                                                                  |
| 19  | Inharmonic string | **Port**       | `$mi.string`                                                                                                                                                                     |
| 20  | Modal resonator   | **Port**       | `$mi.modal`                                                                                                                                                                      |
| 21  | Bass drum         | **Port**       | `$mi.kick` + `$mi.synthKick`                                                                                                                                                     |
| 22  | Snare             | **Port**       | `$mi.snare` + `$mi.synthSnare`                                                                                                                                                   |
| 23  | Hi-hat            | **Port**       | `$mi.hiHat` + `$mi.ringHat`                                                                                                                                                      |

## 3. Main/aux

Rule: **split** when the two outputs are different algorithms that only share knobs; **keep a second output** when it is a byproduct of the same process or costs little next to the main render; **make it a param** when it is a separate, comparably expensive render (Operator computes every output whether or not it is patched); **drop it** when it is a trivial transform of the main output.

| Engine           | Decision                                                                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Phase distortion | Param `sync` (default on). Aux is a second full 2×-oversampled render of the modulator without sync.                           |
| Six-op           | One output (aux = main).                                                                                                       |
| Wave terrain     | Outputs `output` + `pm` (sine phase-modulated by path + height; needs the internal path).                                      |
| Waveshaping      | Outputs `output` + `overtones` (triangle → sine ↔ second folder; one cheap oscillator and a LUT read).                          |
| Two-op FM        | Outputs `output` + `sub` (sub-octave sine PM'd by the carrier, computed in the same oversampled loop).                          |
| Granular formant | Split: `$mi.formant` (grainlet pair) and `$mi.zOsc` (windowed-sine "filtered waveform", peak/LP/BP/HP morph).                  |
| Additive         | Param `series: 'integer' \| 'organ'` (24 vs 8 harmonics, separate oscillator banks).                                            |
| Speech           | Split by model: `$mi.vowel` (formant → SAM → LPC blend) and `$mi.speak` (LPC word banks). Each keeps `excitation` (byproduct). |
| Swarm            | Outputs `output` (saw) + `sine` (same voices and envelopes).                                                                   |
| Particle         | Outputs `output` + `raw` (the dust driving the filters).                                                                       |
| String / Modal   | Outputs `output` + `exciter` (internal excitation, for mixing attack back in).                                                 |
| Drums            | Split analog/synthetic kick and snare; split the two hi-hat models (different noise source, VCA and filter).                   |

## 4. Modules

Parameter names are semantic (no harmonics/timbre/morph). Absolute frequencies (e.g. `$mi.formant`'s `formant`) are V/Oct; knob-style params, including frequencies relative to the pitch (e.g. `$mi.zOsc`'s `cutoff`), are 0–5 V with the Plaits midpoint as default unless noted. Pitch conversions are exact (`exp2`), not Plaits' quantised `NoteToFrequency`/`SemitonesToRatio` tables; outputs ±5 V; every module is polyphonic per channel; knob edits glide via `SwapGlide` as on other oscillators.

**Pitch and FM.** Every pitched module takes `freq` (V/Oct) plus `fm` and `fmMode` (`throughZero` default, `lin`, `exp`) through `apply_fm`, exactly like `$sine`/`$saw`. How each family consumes the modulated frequency:

- Phase-accumulator oscillators (`formant`, `zOsc`, `twoOp`, `phaseDistortion`, `waveshaper`, `waveTerrain`, `additive`, `divideDown`, `swarm`, `vowel`, `speak`, `sixOp`): frequency is read every sample, so audio-rate FM works (Plaits only interpolates per block). Through-zero runs the phase backward; oscillators that reset on wrap (grainlet, Z, SAM, BLEP saws) detect the wrap in both directions and mirror `reset_time`, so the BLEP correction stays valid. Synced inner oscillators (the grainlet's and Z's formant sine) run with the carrier's direction and restart at the phase a forward cycle ends on, so a steady backward waveform is the forward one mirrored.
- Oversampled cores (`twoOp` 4×, `phaseDistortion` 2×, `waveTerrain` 2×): the frequency is held across the substeps of one output sample.
- Resonators and drums (`string`, `modal`, `particles`, drums): frequency sets filter/delay coefficients, which are recomputed at the control rate (§6), so FM there is control-rate. Negative frequency has no meaning for a resonator; `throughZero` behaves as `lin` and this is documented on those modules.

**Excitation and accent.** Modules whose Plaits engine used `level` as accent (six-op velocity, string, modal, speech word replay, drums) take `accent` (0–5 V, default 4 V = Plaits' unpatched 0.8), matching `$tr808.*`. Plaits' unpatched-trigger "sustain" modes (string/modal bowed excitation, drum drones, six-op envelope scrubbing, swarm continuous mode) keep that behaviour when `trigger`/`gate` is disconnected, documented per module.

| Module                 | Source                                           | Params                                                                                      | Inputs                                   | Outputs              |
| ---------------------- | ------------------------------------------------ | ------------------------------------------------------------------------------------------- | ---------------------------------------- | -------------------- |
| `$mi.phaseDistortion`  | `engine2/phase_distortion_engine`                | `ratio` (quantised modulator ratio), `amount`, `asymmetry`, `sync` (bool)                    | `freq`, `fm`                             | `output`             |
| `$mi.sixOp`            | `engine2/six_op_engine` + `fm/*`                 | `bank` (0–2 built-in), `patch` (0–31), `brightness`, `envelope` (time scale / scrub)         | `freq`, `fm`, `gate`, `accent`           | `output`             |
| `$mi.waveTerrain`      | `engine2/wave_terrain_engine`                    | `terrain` (morph across 8), `radius`, `offset`                                              | `freq`, `fm`                             | `output`, `pm`       |
| `$mi.waveshaper`       | `engine/waveshaping_engine` + `lut_ws_*`, `lut_fold*` | `shape` (4-table morph), `fold`, `asymmetry`                                            | `freq`, `fm`                             | `output`, `overtones` |
| `$mi.twoOp`            | `engine/fm_engine`                               | `ratio` (quantised), `index`, `feedback` (bipolar: phase ↔ self feedback)                   | `freq`, `fm`                             | `output`, `sub`      |
| `$mi.formant`          | `oscillator/grainlet_oscillator` ×2              | `formant`, `ratio`, `width`                                                                 | `freq`, `fm`                             | `output`             |
| `$mi.zOsc`             | `oscillator/z_oscillator`                        | `cutoff`, `shape`, `mode`                                                                   | `freq`, `fm`                             | `output`             |
| `$mi.additive`         | `engine/additive_engine` + `harmonic_oscillator` | `centroid`, `slope`, `bumps`, `series`                                                      | `freq`, `fm`                             | `output`             |
| `$mi.vowel`            | `speech/naive_*`, `sam_*`, `lpc_*`               | `model`, `vowel`, `formantShift`                                                            | `freq`, `fm`, `trigger`                  | `output`, `excitation` |
| `$mi.speak`            | `speech/lpc_speech_synth_controller` + words     | `bank`, `word`, `formantShift`, `prosody`, `speed`                                          | `freq`, `fm`, `trigger`, `accent`        | `output`, `excitation` |
| `$mi.swarm`            | `engine/swarm_engine`                            | `density`, `spread`, `grainSize`                                                            | `freq`, `fm`, `trigger`                  | `output`, `sine`     |
| `$mi.particles`        | `engine/particle_engine` + `fx/diffuser`         | `density`, `spread`, `resonance` (bipolar: diffusion ↔ resonance)                           | `freq`, `fm`, `trigger`                  | `output`, `raw`      |
| `$mi.string`           | `physical_modelling/string_voice` + `string`     | `structure`, `brightness`, `damping`                                                        | `freq`, `fm`, `trigger`, `accent`, `input` | `output`, `exciter` |
| `$mi.modal`            | `physical_modelling/modal_voice` + `resonator`   | `structure`, `brightness`, `damping`                                                        | `freq`, `fm`, `trigger`, `accent`, `input` | `output`, `exciter` |
| `$mi.divideDown`       | `oscillator/string_synth_oscillator` + registration | `registration` (organ → string)                                                          | `freq`, `fm`                             | `output`             |
| `$mi.chorus`           | `fx/ensemble`                                    | `amount`, `depth`                                                                           | `input` (mono or stereo)                 | stereo `output`      |
| `$mi.kick`             | `drums/analog_bass_drum` + overdrive             | `tone`, `decay`, `attackFm`, `selfFm`, `drive`                                              | `freq`, `fm`, `trigger`, `accent`        | `output`             |
| `$mi.synthKick`        | `drums/synthetic_bass_drum`                      | `tone`, `decay`, `dirt`, `fmEnvelope`, `fmDecay`                                            | `freq`, `fm`, `trigger`, `accent`        | `output`             |
| `$mi.snare`            | `drums/analog_snare_drum`                        | `tone`, `decay`, `snappy`                                                                   | `freq`, `fm`, `trigger`, `accent`        | `output`             |
| `$mi.synthSnare`       | `drums/synthetic_snare_drum`                     | `tone`, `decay`, `snappy`                                                                   | `freq`, `fm`, `trigger`, `accent`        | `output`             |
| `$mi.hiHat`            | `drums/hi_hat` (square noise, swing VCA, HPF)    | `tone`, `decay`, `noisiness`                                                                | `freq`, `fm`, `trigger`, `accent`        | `output`             |
| `$mi.ringHat`          | `drums/hi_hat` (ring-mod noise, linear VCA)      | `tone`, `decay`, `noisiness`                                                                | `freq`, `fm`, `trigger`, `accent`        | `output`             |

Notes:

- `$mi.string`/`$mi.modal` add an `input` for external excitation (as on Rings). The internal exciter fires on `trigger`; `input` sums into the resonator.
- The drums' exact param split (Plaits folds several controls into one knob, e.g. `harmonics` → attack FM then self FM then drive on the analog kick) is decided per module from the engine's mapping; the names above are the intent.
- `$mi.sixOp` uses `gate`, not `trigger`, because DX envelopes sustain while held. It keeps Plaits' two-voice alternation per channel so a retrigger never cuts a release tail.

### What `$macro`'s extra params become

- **LPG (`lpgColor`, `lpgDecay`, `level` as VCA):** none of the new modules — patch `$lpg` after them.
- **`level` as accent:** `accent` on the modules listed above.
- **`timbreAmt` / `morphAmt` / `fmAmt` + internal decay envelope:** not needed — `$perc`/`$adsr` into any param; FM is `fm`. Speech's engine-specific uses become `prosody` and `speed` on `$mi.speak`.
- **`mix`:** `$mix`.

### Not ported (embedded-only)

Staggered six-op rendering; `kCorrectedSampleRate` (47872.34 Hz crystal correction); `BufferAllocator`; int16 output / `Clip16`; user flash data (user wavetables, terrain, SYX upload); engine quantizer + engine CV; pot `ONE_POLE` smoothing (`SwapGlide`); 1 ms trigger delay; `already_enveloped` plumbing and the post-processor LPG. Each engine's output gain and, for negative gains, the `Limiter` (speech, swarm, particle, string, modal) are kept — they are part of the sound.

## 5. `$chord` inversion

Port `ChordBank::ComputeChordInversion` generalised from 4 notes to N:

- New `inversion` signal (default 0): the number of chord notes raised an octave, continuous. Fractional values crossfade the note in motion between its two octaves.
- When `inversion` is connected the output has **N+1 channels** (the extra voice carries the crossfading note) and a new `gain` output gives each channel's amplitude. Voices rotate rather than re-sort, so each channel's pitch changes only by octave jumps under a zero-gain crossfade — no glides or clicks in downstream oscillators.
- Negative values drop notes an octave, mirroring the positive direction.
- Disconnected, `$chord` is unchanged (N channels, `gain` all 5 V).
- `gain` is a 0–5 V level (5 V = full), like the other knob-style signals.

## 6. Execution model

### Control clock + per-sample state, with a block path

Plaits computes control values once per 12-sample block at 48 kHz (4 kHz) and ramps them with `ParameterInterpolator`; `$macro` therefore buffers 12 samples of output. The ports instead run Operator-style:

- Each channel holds a **control clock** at a fixed 4 kHz (`phase += 4000 / sample_rate`). When it wraps, `tick()` reads the control inputs at that sample, recomputes control-rate state (filter coefficients, envelope steps, LPC frames, swarm envelopes, harmonic amplitudes, DX envelopes and LFO), and sets per-sample increments that ramp each interpolated value to its new target by the next tick — the `ParameterInterpolator` behaviour without a buffer.
- `step()` produces one output sample from per-sample state. Frequency/FM are read here, every sample.
- `update` = `if due { tick() }; step()`. No output latency (`$macro` has 12 samples).
- `process_block` splits its range at tick boundaries and runs `tick()` + a chunk loop. Heavy multi-stage cores run **stage-major** within the chunk (all six DX operators one after another over the chunk; all modal SVF modes over the chunk; hi-hat noise → VCA → filter as passes), which keeps state in registers and vectorises, while each sample still sees the same arithmetic in the same order, so the paths agree bit for bit (checked by `tests/block_<module>.rs` with `tests/block_common`).
- At 48 kHz the tick lands every 12 samples from sample 0, aligned with Plaits' blocks, so the reference tests (§8) compare against C++ directly.

Per-module feasibility — every core decomposes this way; none needs an output buffer:

| Module                | Per-sample `step()`                             | Block path worth measuring                                            |
| --------------------- | ----------------------------------------------- | --------------------------------------------------------------------- |
| phaseDistortion       | yes (2 substeps + naive decimation)             | yes: constant ratio/amount hoisted                                    |
| sixOp                 | yes (sample-major over 6 ops)                   | **yes**: operator-major chunks                                         |
| waveTerrain           | yes (2 substeps, terrain eval)                  | yes                                                                   |
| waveshaper            | yes                                             | yes: shaper table pair selected once per chunk                        |
| twoOp                 | yes (4 substeps + FIR downsampler per sample)   | yes                                                                   |
| formant, zOsc         | yes                                             | yes: cheap modules, input reads dominate                               |
| additive              | yes (Chebyshev recurrence per harmonic)         | **yes**: harmonic-major chunks                                         |
| vowel, speak          | yes (LPC clock is already a phase accumulator)  | no — per-sample work dominates                                        |
| swarm                 | yes (8 voices × saw + sine)                     | yes: voice-major chunks                                                |
| particles             | yes                                             | yes                                                                   |
| string                | yes (delay line + filters)                      | no — memory reads dominate (as with `$grains`)                         |
| modal                 | yes (up to 24 SVF modes)                        | **yes**: mode-major chunks                                             |
| divideDown            | yes                                             | yes                                                                   |
| chorus                | yes                                             | no — delay-line reads dominate                                        |
| drums                 | yes                                             | hi-hats yes (stage-major); others measure                             |

Block paths are added only where `cargo bench -p modular_core --bench patch_bench` and real patches show a gain.

### Sample rate: rate-correct ports, no resampler

Running cores at 48 kHz behind a resampler would cost a polyphase kernel per channel per module, so instead every rate-dependent constant is converted at port time. Categories to audit in each file:

- `kSampleRate` / `kCorrectedSampleRate` uses (`NoteToFrequency`, SAM formant increments, click/consonant durations, LPC frame length) → `sample_rate`.
- Per-block coefficients (one-pole rates, envelope decays, `short_decay`-style constants) → stay per tick; the 4 kHz control clock keeps their meaning at any rate.
- Per-sample coefficients written as numbers (`ONE_POLE(x, y, 0.05f)` in audio loops, pulse colouring filters, DC blockers) → converted to time constants and recomputed for the rate in `init`.
- Delay lengths in samples (diffuser, ensemble, string) → scaled by `sample_rate / 48000` and allocated in `init`.
- Oversampling FIRs (`lut_4x_downsampler_fir`) are rate-relative and stay as is.
- Frequency limits in cycles per sample (`kMaxFrequency` and fractions of it) → their 48 kHz value in Hz (`max_frequency(sample_rate)`).

At 48 kHz the converted constants reproduce the C++ values exactly, so the reference comparison is unaffected; at 44.1 and 96 kHz each module is checked against its own 48 kHz render (§8).

### Efficiency

- Control-rate work only on ticks, and skipped when its inputs haven't changed (`changed()` caching as in `$lpg`).
- Plaits' `Sine` LUT ported as-is (needed for bit-near output, and cheaper than `sin`).
- Work Plaits repeats across parallel oscillators with identical inputs is done once (the granular engine's two grainlets share one carrier window).
- Tables as generated `static` arrays; the three six-op banks are unpacked on the main thread (in `init`, into a process-wide cache shared by all instances), so selecting a patch on the audio thread is a pointer swap.
- Modal always runs 24 modes; modes above Nyquist are clamped to 0.499 with ~0.2% amplitude rather than dropped. Skipping them is a measurable saving at high pitch but no longer bit-exact — an option to measure, not a default.
- No allocation outside `init`; each module gets a `minimal_params` case in `dsp_fresh_tests` and passes the alloc-detector gate.
- No SIMD across channels without a full-engine measurement on real patches first.

## 7. Shared code

`crates/modular_core/src/dsp/mi/` — one file or directory per module, plus `mi/core/` for ported primitives: `Svf`/`OnePole`, polyBLEP, LUT `Sine`/`SinePM`, `SemitonesToRatio`, `Limiter`, `Downsampler`, `Random`/`Dust` (same LCG as stmlib, seeded per channel), `DelayLine`, `HysteresisQuantizer` (patch/word selection), `Diffuser`, `Ensemble`, `Resonator`, `String`, chord registration. Tables (`lut_sine`, `lut_fm_frequency_quantizer`, `lut_fold`, `lut_fold_2`, `lut_ws_*`, `lut_stiffness`, `lut_svf_shift`, `lut_4x_downsampler_fir`, `syx_bank_0..2`, speech/SAM/LPC data) are generated from the C++ resources by a script, not copied by hand. `$lpg` moves onto `mi::core::Svf`.

## 8. Verification

1. `scripts/mi-reference/`: `fetch.sh` checks out `pichenettes/eurorack` and `stmlib` at pinned commits; `generate_tables.py` writes `mi/core/tables.rs`; `build.sh` compiles each `drivers/*.cc` (48 kHz, 12-sample blocks, `-ffp-contract=off`) and writes `crates/modular_core/tests/fixtures/mi/<driver>.f32`. A driver is the engine's `Render` with Operator's control mapping (knobs in volts, exact pitch conversions), and each record holds a block's inputs then its outputs, so the Rust test replays the record without restating the sequence. Fixtures are committed; the C++ is not.
2. Rust tests drive each voice at 48 kHz with the same sequences (ramping the pitch per block as Plaits does) and compare: ≤1e-5 per sample. Random-driven modules use the stmlib LCG with the same seed, so they are deterministic too.
3. Block/sample agreement and allocation-free tests per block module.
4. Rate checks: renders at 44.1 and 96 kHz against the 48 kHz render — the first 16 harmonics (pitched modules) and envelope times within tolerance.
5. Listen against VCV Plaits at matched settings before calling each module done.

## 9. Order

1. `mi/core` primitives, table generator, reference harness; `$lpg` onto the in-tree SVF without the decay clamp.
2. `$mi.formant`, `$mi.zOsc` (the reported buzz).
3. `$mi.twoOp`, `$mi.phaseDistortion`, `$mi.waveshaper`.
4. `$mi.modal`, `$mi.string`.
5. `$mi.additive`, `$mi.waveTerrain`, `$mi.swarm`, `$mi.particles`.
6. `$mi.divideDown`, `$mi.chorus`, `$chord` inversion.
7. `$mi.sixOp`.
8. `$mi.vowel`, `$mi.speak`.
9. Drums.
10. Remove `$macro`, the `mi-plaits-dsp` submodule, workspace member and `extern crate` — a breaking change in its own changeset, with doc examples for the replicable engines in §2.
