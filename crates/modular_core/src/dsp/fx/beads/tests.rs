use crate::poly::{MonoSignal, PolySignal};
use crate::types::Signal;

pub(super) const SR: f32 = 48000.0;

pub(super) fn volts(v: f32) -> Option<MonoSignal> {
    Some(MonoSignal::from_poly(PolySignal::mono(Signal::Volts(v))))
}

pub(super) fn poly_volts(vs: &[f32]) -> Option<PolySignal> {
    let signals: Vec<Signal> = vs.iter().map(|&v| Signal::Volts(v)).collect();
    Some(PolySignal::poly(&signals))
}

pub(super) fn sine(freq: f32, n: usize, sample_rate: f32, amp: f32) -> f32 {
    amp * (std::f32::consts::TAU * freq * n as f32 / sample_rate).sin()
}

pub(super) fn rms(xs: &[f32]) -> f32 {
    (xs.iter().map(|x| x * x).sum::<f32>() / xs.len() as f32).sqrt()
}
