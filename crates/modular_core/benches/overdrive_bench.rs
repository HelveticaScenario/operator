use criterion::{Criterion, criterion_group, criterion_main};
use modular_core::dsp::fx::overdrive::{
    OverdriveMode, OverdriveParams, __bench_make_overdrive,
};
use modular_core::poly::PolySignal;
use modular_core::types::Signal;
use std::hint::black_box;

fn p(input: f32, drive: f32, tone: Option<f32>, mode: OverdriveMode) -> OverdriveParams {
    OverdriveParams {
        input: PolySignal::mono(Signal::Volts(input)),
        drive: PolySignal::mono(Signal::Volts(drive)),
        tone: tone.map(|t| PolySignal::mono(Signal::Volts(t))),
        mode: Some(mode),
    }
}

fn bench_overdrive(c: &mut Criterion) {
    let mut g = c.benchmark_group("overdrive");

    for (label, mode) in &[
        ("soft", OverdriveMode::Soft),
        ("hard", OverdriveMode::Hard),
        ("asym", OverdriveMode::Asym),
    ] {
        let mut od = __bench_make_overdrive(p(1.5, 2.5, None, *mode));
        g.bench_function(format!("{label}_notone"), |b| {
            b.iter(|| {
                od.__bench_update(black_box(48000.0));
            })
        });

        let mut od = __bench_make_overdrive(p(1.5, 2.5, Some(2.0), *mode));
        g.bench_function(format!("{label}_tone"), |b| {
            b.iter(|| {
                od.__bench_update(black_box(48000.0));
            })
        });
    }

    g.finish();
}

criterion_group!(benches, bench_overdrive);
criterion_main!(benches);
