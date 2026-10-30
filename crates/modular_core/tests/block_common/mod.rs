//! Shared harness for `#[module(block)]` tests. A block module must produce the
//! same samples whichever way its inputs resolve — whole-block rows, inputs
//! copied out slot by slot, or the per-sample `update` in a feedback cycle —
//! and must not touch the heap on any of those paths.
//!
//! Each block module's test file uses the parts it needs, so items unused by a
//! given test binary are expected.
#![allow(dead_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use modular_core::Patch;
use modular_core::dsp::get_params_deserializers;
use modular_core::params::DeserializedParams;
use modular_core::types::{ProcessingMode, SampleBuffer, WavData};
use serde_json::{Value, json};

pub const SAMPLE_RATE: f32 = 48000.0;
pub const BLOCK_SIZE: usize = 64;
pub const CHANNELS: usize = 4;
pub const BLOCKS: usize = 4;

pub fn cable(module: &str, channel: usize) -> Value {
    json!({ "type": "cable", "module": module, "port": "output", "channel": channel })
}

pub fn poly_cable(module: &str, channels: usize) -> Value {
    Value::Array((0..channels).map(|ch| cable(module, ch)).collect())
}

/// `src` (a `CHANNELS`-wide saw) and `lfo` (a `CHANNELS`-wide sine) feed
/// `dut`, a `module_type` with `params`. The wav `table` holds [`wavetable`].
pub fn build(
    src_mode: ProcessingMode,
    dut_mode: ProcessingMode,
    module_type: &str,
    params: Value,
) -> Patch {
    let deserializers = get_params_deserializers();
    let freqs: Vec<f32> = (0..CHANNELS).map(|ch| ch as f32 * 0.3).collect();
    let lfo_freqs: Vec<f32> = (0..CHANNELS).map(|ch| ch as f32 * 0.2 - 3.0).collect();
    let modules = [
        ("src", "$saw", json!({ "freq": freqs }), src_mode),
        ("lfo", "$sine", json!({ "freq": lfo_freqs }), src_mode),
        ("dut", module_type, params, dut_mode),
    ];
    let mut patch = Patch::new();
    patch
        .insert_modules(
            modules.into_iter().map(|(id, module_type, params, mode)| {
                let cached = deserializers[module_type](params).unwrap();
                (
                    id.to_string(),
                    module_type.to_string(),
                    DeserializedParams {
                        params: cached.params,
                        channel_count: cached.channel_count,
                    },
                    mode,
                )
            }),
            SAMPLE_RATE,
            BLOCK_SIZE,
        )
        .unwrap();
    patch.wav_data.insert("table".into(), wavetable());
    for module in patch.sampleables.values() {
        module.prepare_resources(&patch.wav_data);
    }
    patch.connect_all();
    patch
}

/// A 16-frame wavetable of 256-sample cycles, each frame brighter than the last.
pub fn wavetable() -> Arc<WavData> {
    const SIZE: usize = 256;
    let samples = (0..16)
        .flat_map(|frame| {
            (0..SIZE).map(move |i| {
                let phase = std::f32::consts::TAU * i as f32 / SIZE as f32;
                (phase.sin() + (phase * (2 + frame) as f32).sin() * 0.3) * 0.7
            })
        })
        .collect();
    Arc::new(WavData::new(
        SampleBuffer::from_samples(vec![samples], 48_000.0),
        Some(SIZE),
    ))
}

/// Advance every module through `steps` (ascending slot targets ending at
/// `BLOCK_SIZE`) for one block, `dut` first so its reads pull unrendered
/// producers.
fn render_block(patch: &Patch, steps: &[usize]) {
    for module in patch.sampleables.values() {
        module.start_block();
    }
    for &step in steps {
        for id in ["dut", "src", "lfo"] {
            patch.sampleables[id].ensure_processed_to(step);
        }
    }
}

/// Render `BLOCKS` blocks through `steps`. Returns `(dut, src, lfo)` samples,
/// slot major then channel.
pub fn render(patch: &Patch, steps: &[usize]) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut out = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..BLOCKS {
        render_block(patch, steps);
        for index in 0..BLOCK_SIZE {
            for ch in 0..CHANNELS {
                let read = |id: &str| patch.sampleables[id].get_value_at("output", ch, index);
                out.0.push(read("dut"));
                out.1.push(read("src"));
                out.2.push(read("lfo"));
            }
        }
    }
    out
}

/// `value` repeated for every rendered sample.
pub fn constant(value: f32) -> Vec<f32> {
    vec![value; BLOCKS * BLOCK_SIZE * CHANNELS]
}

/// `f(src, lfo)` per sample, silencing non-finite results like every output.
pub fn elementwise(src: &[f32], lfo: &[f32], f: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    src.iter()
        .zip(lfo)
        .map(|(&x, &y)| f(x, y))
        .map(|v| if v.is_finite() { v } else { 0.0 })
        .collect()
}

const STEP_PATTERNS: [&[usize]; 2] = [&[BLOCK_SIZE], &[1, 10, 11, 40, BLOCK_SIZE]];

/// Every sample of `dut` under each processing mode and step pattern, checked
/// against `reference(src, lfo)`.
pub fn check_module(
    module_type: &str,
    params: Value,
    reference: impl Fn(&[f32], &[f32]) -> Vec<f32>,
) {
    let modes = [
        (ProcessingMode::Block, ProcessingMode::Block),
        (ProcessingMode::Sample, ProcessingMode::Block),
        (ProcessingMode::Block, ProcessingMode::Sample),
    ];
    for (src_mode, dut_mode) in modes {
        for steps in STEP_PATTERNS {
            let patch = build(src_mode, dut_mode, module_type, params.clone());
            let (dut, src, lfo) = render(&patch, steps);
            assert_eq!(
                dut,
                reference(&src, &lfo),
                "{module_type}: src {src_mode:?}, dut {dut_mode:?}, steps {steps:?}"
            );
        }
    }
}

/// Every sample of `dut` under each processing mode and step pattern matches
/// the per-sample `update` path (`dut` in Sample mode).
pub fn check_paths_agree(module_type: &str, params: Value) {
    let reference = build(
        ProcessingMode::Block,
        ProcessingMode::Sample,
        module_type,
        params.clone(),
    );
    let (expected, _, _) = render(&reference, &[BLOCK_SIZE]);
    assert!(
        expected.iter().any(|&v| v != 0.0),
        "{module_type} {params} renders silence"
    );
    let modes = [
        (ProcessingMode::Block, ProcessingMode::Block),
        (ProcessingMode::Sample, ProcessingMode::Block),
    ];
    for (src_mode, dut_mode) in modes {
        for steps in STEP_PATTERNS {
            let patch = build(src_mode, dut_mode, module_type, params.clone());
            let (dut, _, _) = render(&patch, steps);
            assert_eq!(
                dut, expected,
                "{module_type} {params}: src {src_mode:?}, steps {steps:?}"
            );
        }
    }
}

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// Counts heap allocations and frees made on a thread while it is inside
/// [`allocations_while_rendering`]; other threads (parallel tests) are not
/// counted. A test binary opts in with
/// `#[global_allocator] static GLOBAL: CountingAllocator = CountingAllocator;`.
pub struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            ALLOCATIONS.with(|n| n.set(n.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.with(Cell::get) {
            ALLOCATIONS.with(|n| n.set(n.get() + 1));
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// Heap allocations and frees made while rendering `dut` (built outside the
/// count) for several blocks in Block mode, from Sample-mode producers, and
/// in Sample mode itself, through every step pattern. Requires the binary's
/// global allocator to be [`CountingAllocator`].
pub fn allocations_while_rendering(module_type: &str, params: Value) -> usize {
    let modes = [
        (ProcessingMode::Block, ProcessingMode::Block),
        (ProcessingMode::Sample, ProcessingMode::Block),
        (ProcessingMode::Block, ProcessingMode::Sample),
    ];
    let patches: Vec<Patch> = modes
        .iter()
        .map(|&(src_mode, dut_mode)| build(src_mode, dut_mode, module_type, params.clone()))
        .collect();
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|c| c.set(true));
    for patch in &patches {
        for steps in STEP_PATTERNS {
            for _ in 0..BLOCKS {
                render_block(patch, steps);
            }
        }
    }
    COUNTING.with(|c| c.set(false));
    ALLOCATIONS.with(Cell::get)
}
