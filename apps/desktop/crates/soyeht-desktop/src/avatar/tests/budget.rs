//! The frame budget: how long one frame takes to draw, on the machine the test runs on. The
//! tests are `#[ignore]`d (a timing is of no use on a busy gate host); run them in a release build
//! where the number matters, and keep the output with the machine's name in the report:
//! `cargo test --release -p soyeht-desktop avatar::tests::budget -- --ignored --nocapture`.

use super::synthetic::Spec;
use crate::avatar::{
    Rasteriser,
    resample::{Field, resample},
};
use soyeht_avatar::{Audio, Frame, Runtime, State, Tune};
use std::{sync::Arc, time::Instant};

/// One frame of a 60 Hz screen: the avatar alone must fit in it, and the aim is half, leaving the
/// rest to the window.
const BUDGET_MS: f64 = 1000.0 / 60.0;
const FRAMES: usize = 600;

/// The frames of a speaking avatar, stepped at 60 Hz by the energy of a made-up voice.
fn speaking(runtime: &mut Runtime) -> Vec<Frame> {
    (0..FRAMES)
        .map(|i| {
            let t = i as f64 / 60.0;
            let db = -40.0 + 22.0 * (0.5 + 0.5 * (t * 22.0).sin());
            let audio = Some(Audio {
                position: t,
                db: Some(db),
            });
            runtime.step(1.0 / 60.0, State::Speaking, audio)
        })
        .collect()
}

fn mean(times: &[f64]) -> f64 {
    times.iter().sum::<f64>() / times.len() as f64
}

/// Draws a speaking avatar and returns the time of each frame in milliseconds, sorted.
fn timings(spec: Spec, width: u32) -> Vec<f64> {
    let avatar = Arc::new(spec.avatar());
    let triangles: usize = avatar
        .package()
        .mouth
        .parts
        .iter()
        .map(|part| part.triangles.len() / 3)
        .sum();
    let mut raster = Rasteriser::new(Arc::clone(&avatar));
    let frames = speaking(&mut Runtime::new(avatar.package(), Tune::default(), 3));
    let mut times: Vec<f64> = frames
        .iter()
        .map(|frame| {
            let started = Instant::now();
            std::hint::black_box(raster.render(frame, width).bgra);
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    println!(
        "avatar frame, canvas {side}, drawn {width} wide, {triangles} triangles in a layer: \
         mean {mean:.2} ms, median {median:.2}, p95 {p95:.2}, worst {worst:.2}",
        side = spec.side,
        mean = mean(&times),
        median = times[times.len() / 2],
        p95 = times[times.len() * 95 / 100],
        worst = times[times.len() - 1],
    );
    times
}

#[test]
#[ignore = "a timing, for a release build on a quiet machine"]
fn frame_budget_at_512() {
    let spec = Spec {
        side: 512,
        ring: 256,
    };
    let mean = mean(&timings(spec, 512));
    assert!(
        mean < BUDGET_MS,
        "{mean:.2} ms is over the {BUDGET_MS:.2} ms budget"
    );
}

#[test]
#[ignore = "a timing, for a release build on a quiet machine"]
fn frame_cost_by_size() {
    for (side, width) in [(512, 256), (1024, 512), (1024, 1024)] {
        timings(Spec { side, ring: 256 }, width);
    }
}

/// Where the time of a 512 pixel frame goes: the picture composed from the base, the eyes and the
/// mouth; the rule evaluated on the grid; the read of the picture through it.
#[test]
#[ignore = "a timing, for a release build on a quiet machine"]
fn frame_cost_by_step() {
    let spec = Spec {
        side: 512,
        ring: 256,
    };
    let avatar = Arc::new(spec.avatar());
    let mut raster = Rasteriser::new(Arc::clone(&avatar));
    let frames = speaking(&mut Runtime::new(avatar.package(), Tune::default(), 3));
    let started = Instant::now();
    for frame in &frames {
        raster.compose(frame);
    }
    println!(
        "compose: {:.2} ms",
        started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64
    );
    let package = avatar.package();
    let canvas = (package.canvas.width, package.canvas.height);
    let mut field = Field::default();
    let started = Instant::now();
    for frame in &frames {
        field.fill(&package.warp, &frame.acting.pose, canvas, (512, 512));
    }
    println!(
        "grid of the rule: {:.2} ms",
        started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64
    );
    let mut out = vec![0; 512 * 512 * 4];
    let started = Instant::now();
    for _ in 0..FRAMES {
        resample(&field, avatar.base(), (512, 512), &mut out);
    }
    println!(
        "read through the grid: {:.2} ms",
        started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64
    );
}
