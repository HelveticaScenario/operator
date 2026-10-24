use super::super::channel::grid_points;
use super::*;

#[test]
fn grid_points_start_at_one_stage_and_end_at_fifty() {
    assert_eq!(grid_points(1.0, 8), (1, 9));
    assert_eq!(grid_points(13.0, 8), (9, 17));
    assert_eq!(grid_points(49.5, 8), (49, 50));
    assert_eq!(grid_points(50.0, 49), (50, 50));
    assert_eq!(grid_points(3.75, 1), (3, 4));
    assert_eq!(grid_points(3.25, 2), (3, 5));
}

#[test]
fn crossfade_blends_the_chains_either_side() {
    // (stages, fade step, lower chain, upper chain, upper's share)
    for (stages, step, lower, upper, mix) in [
        (13.0, 8, 9.0, 17.0, 0.5),
        (1.25, 1, 1.0, 2.0, 0.25),
        (3.5, 2, 3.0, 5.0, 0.25),
        (49.5, 8, 49.0, 50.0, 0.5),
    ] {
        let response = |stages, step| {
            faded_impulse_response(controls(0.5, stages, 2.0), step, 2048, &mut channel())
        };
        let (below, above) = (response(lower, 0), response(upper, 0));
        let faded = response(stages, step);
        for n in 0..2048 {
            let expected = below[n] + mix * (above[n] - below[n]);
            assert!(
                (faded[n] - expected).abs() < 1e-5,
                "{stages} stages at step {step}, sample {n}"
            );
        }
    }
}

#[test]
fn crossfade_keeps_the_level_above_the_cutoff() {
    // A sine four octaves above the cutoff, halfway between grid points.
    for (stages, step) in [(1.5, 1), (4.0, 2)] {
        let mut ch = channel();
        let c = controls(-4.0, stages, 0.0);
        let y: Vec<f32> = (0..SR as usize)
            .map(|n| {
                let x = (std::f32::consts::TAU * C4_HZ_F32 * n as f32 / SR).sin();
                ch.step(x, c, step, log2_c4_over_fs())
            })
            .collect();
        let level = rms(&y, 0.5, 1.0) * std::f32::consts::SQRT_2;
        assert!(
            level > 0.95,
            "{stages} stages at step {step}: level {level}"
        );
    }
}

#[test]
fn crossfade_at_a_grid_point_is_that_chain() {
    for (stages, step) in [(9.0, 8), (3.0, 2), (50.0, 49)] {
        let c = controls(0.0, stages, 3.0);
        assert_eq!(
            faded_impulse_response(c, step, 1024, &mut channel()),
            faded_impulse_response(c, 0, 1024, &mut channel()),
            "{stages} stages at step {step}"
        );
    }
}

#[test]
fn crossfade_sweeps_across_grid_points_without_jumps() {
    // A low sine while `stages` sweeps up over several grid points.
    for step in [0, 1, 2, 8] {
        let mut ch = channel();
        let len = SR as usize;
        let mut previous = 0.0;
        let mut largest = 0.0f32;
        for n in 0..len {
            let x = (std::f32::consts::TAU * 50.0 * n as f32 / SR).sin();
            let stages = 1.0 + 20.0 * n as f32 / len as f32;
            let y = ch.step(x, controls(0.0, stages, 1.0), step, log2_c4_over_fs());
            if n > 0 {
                largest = largest.max((y - previous).abs());
            }
            previous = y;
        }
        // A 50 Hz sine moves at most 0.0065 per sample.
        assert!(largest < 0.02, "step {step}: largest step {largest}");
    }
}

#[test]
fn fade_step_is_at_most_49() {
    let deserializers = crate::dsp::get_params_deserializers();
    let de = deserializers.get("$apf").unwrap();
    let with = |fade_step: u32| {
        de(serde_json::json!({ "input": 0.0, "cutoff": 0.0, "fadeStep": fade_step }))
    };
    for fade_step in [0, 1, 8, 49] {
        assert!(with(fade_step).is_ok(), "fadeStep {fade_step}");
    }
    assert!(with(50).is_err());
}

#[test]
fn crossfade_moves_off_a_grid_point_without_a_jump() {
    // Moving off a grid point brings the upper chain in settled, so the
    // output stays continuous.
    let mut ch = channel();
    let len = SR as usize;
    let mut previous = 0.0;
    let mut largest = 0.0f32;
    for n in 0..len {
        let x = (std::f32::consts::TAU * 50.0 * n as f32 / SR).sin();
        let stages = if n < len / 2 {
            9.0
        } else {
            9.0 + (n - len / 2) as f32 / len as f32
        };
        let y = ch.step(x, controls(0.0, stages, 1.0), 8, log2_c4_over_fs());
        if n > 0 {
            largest = largest.max((y - previous).abs());
        }
        previous = y;
    }
    assert!(largest < 0.02, "largest step {largest}");
}
