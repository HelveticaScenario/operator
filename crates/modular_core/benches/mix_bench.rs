use criterion::{Criterion, criterion_group, criterion_main};
use modular_core::dsp::core::mix::{MixMode, MixParams, __bench_make_mix};
use modular_core::poly::PolySignal;
use modular_core::types::Signal;
use std::hint::black_box;

fn mono_volts(n: usize) -> Vec<PolySignal> {
    (0..n)
        .map(|i| PolySignal::mono(Signal::Volts((i as f32) * 0.1)))
        .collect()
}

fn poly_volts(n: usize, channels: usize) -> Vec<PolySignal> {
    (0..n)
        .map(|i| {
            let sigs: Vec<Signal> = (0..channels)
                .map(|c| Signal::Volts((i as f32) * 0.1 + (c as f32) * 0.01))
                .collect();
            PolySignal::poly(&sigs)
        })
        .collect()
}

fn bench_mix(c: &mut Criterion) {
    let mut g = c.benchmark_group("mix");

    for &n in &[2usize, 8, 32] {
        let mut mixer = __bench_make_mix(MixParams {
            inputs: mono_volts(n),
            mode: MixMode::Sum,
            gain: None,
        });
        g.bench_function(format!("mono_sum_n{n}_nogain"), |b| {
            b.iter(|| {
                mixer.__bench_update(black_box(48000.0));
            })
        });

        let mut mixer = __bench_make_mix(MixParams {
            inputs: mono_volts(n),
            mode: MixMode::Sum,
            gain: Some(PolySignal::mono(Signal::Volts(5.0))),
        });
        g.bench_function(format!("mono_sum_n{n}_gain"), |b| {
            b.iter(|| {
                mixer.__bench_update(black_box(48000.0));
            })
        });

        let mut mixer = __bench_make_mix(MixParams {
            inputs: mono_volts(n),
            mode: MixMode::Average,
            gain: None,
        });
        g.bench_function(format!("mono_avg_n{n}_nogain"), |b| {
            b.iter(|| {
                mixer.__bench_update(black_box(48000.0));
            })
        });

        let mut mixer = __bench_make_mix(MixParams {
            inputs: mono_volts(n),
            mode: MixMode::Max,
            gain: None,
        });
        g.bench_function(format!("mono_max_n{n}_nogain"), |b| {
            b.iter(|| {
                mixer.__bench_update(black_box(48000.0));
            })
        });
    }

    let mut mixer = __bench_make_mix(MixParams {
        inputs: poly_volts(8, 8),
        mode: MixMode::Sum,
        gain: Some(PolySignal::mono(Signal::Volts(5.0))),
    });
    g.bench_function("poly_sum_n8_ch8_gain", |b| {
        b.iter(|| {
            mixer.__bench_update(black_box(48000.0));
        })
    });

    g.finish();
}

criterion_group!(benches, bench_mix);
criterion_main!(benches);
