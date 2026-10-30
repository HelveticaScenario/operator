//! Reads the reference renders `scripts/mi-reference/build.sh` writes from the
//! Mutable Instruments C++.

/// The fixture `name` as records of `width` floats.
pub fn records(name: &str, width: usize) -> Vec<Vec<f32>> {
    let path = format!(
        "{}/tests/fixtures/mi/{name}.f32",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let floats: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    assert_eq!(floats.len() % width, 0, "{path} is not records of {width}");
    floats.chunks(width).map(<[f32]>::to_vec).collect()
}

/// Largest absolute difference between `actual` and `expected` sample by
/// sample, with the index where it occurs.
pub fn max_error(actual: &[f32], expected: &[f32]) -> (f32, usize) {
    assert_eq!(actual.len(), expected.len());
    actual
        .iter()
        .zip(expected)
        .map(|(a, e)| (a - e).abs())
        .enumerate()
        .fold(
            (0.0, 0),
            |(max, at), (i, err)| {
                if err > max { (err, i) } else { (max, at) }
            },
        )
}

/// Amplitudes of the first `count` harmonics of `hz` in `x`, rendered at
/// `sample_rate`, measured over its second half.
pub fn harmonics(x: &[f32], hz: f32, sample_rate: f32, count: usize) -> Vec<f32> {
    let x = &x[x.len() / 2..];
    (1..=count)
        .map(|h| {
            let w = std::f64::consts::TAU * h as f64 * hz as f64 / sample_rate as f64;
            let (re, im) = x.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, &v)| {
                let phase = w * n as f64;
                (re + v as f64 * phase.cos(), im + v as f64 * phase.sin())
            });
            (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
        })
        .collect()
}

/// Asserts that `render(sample_rate)`, a tone at `hz`, has the same first 16
/// harmonics at 44.1 and 96 kHz as at 48 kHz, within `tolerance` of the
/// loudest.
pub fn assert_rate_independent(render: impl Fn(f32) -> Vec<f32>, hz: f32, tolerance: f32) {
    let reference = harmonics(&render(48000.0), hz, 48000.0, 16);
    let loudest = reference.iter().fold(0.0f32, |m, &a| m.max(a));
    for sample_rate in [44100.0, 96000.0] {
        let actual = harmonics(&render(sample_rate), hz, sample_rate, 16);
        for (h, (a, r)) in actual.iter().zip(&reference).enumerate() {
            assert!(
                (a - r).abs() < tolerance * loudest,
                "{sample_rate} Hz, harmonic {}: {a} vs {r} at 48 kHz",
                h + 1
            );
        }
    }
}
