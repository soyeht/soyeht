//! The frame budget: how long one frame takes to draw, on the machine the test runs on. The
//! tests are `#[ignore]`d (a timing is of no use on a busy gate host); run them in a release build
//! where the number matters, and keep the output with the machine's name in the report:
//! `cargo test --release -p soyeht-desktop avatar::tests::budget -- --ignored --nocapture`.

use super::{raster_tests::speaking, synthetic::Spec};
use crate::{
    Avatar, Rasteriser,
    resample::{Field, resample},
};
use soyeht_avatar::{Frame, Runtime, Tune};
use std::{sync::Arc, time::Instant};

/// One frame of a 60 Hz screen: the avatar alone must fit in it, and the aim is half, leaving the
/// rest to the window.
const BUDGET_MS: f64 = 1000.0 / 60.0;
const FRAMES: usize = 600;
/// The size of the picture of the usual avatar, in pixels across.
const USUAL: u32 = 2 * 256;

/// An avatar, its rasteriser and the frames of it speaking.
fn setup(spec: Spec) -> (Arc<Avatar>, Rasteriser, Vec<Frame>) {
    let avatar = Arc::new(spec.avatar());
    let raster = Rasteriser::new(Arc::clone(&avatar));
    let mut runtime = Runtime::new(avatar.package(), Tune::default(), 3);
    let frames = speaking(&mut runtime, FRAMES);
    (avatar, raster, frames)
}

/// The time of each run of `work` over the frames, in milliseconds.
fn times(frames: &[Frame], mut work: impl FnMut(&Frame)) -> Vec<f64> {
    frames
        .iter()
        .map(|frame| {
            let started = Instant::now();
            work(frame);
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect()
}

fn mean(times: &[f64]) -> f64 {
    times.iter().sum::<f64>() / times.len() as f64
}

/// Draws a speaking avatar and returns the time of each frame in milliseconds, sorted.
fn timings(spec: Spec, width: u32) -> Vec<f64> {
    let (avatar, mut raster, frames) = setup(spec);
    let triangles: usize = avatar
        .package()
        .mouth
        .parts
        .iter()
        .map(|part| part.triangles.len() / 3)
        .sum();
    let mut all = times(&frames, |frame| {
        std::hint::black_box(raster.render(frame, width).bgra);
    });
    all.sort_by(f64::total_cmp);
    println!(
        "avatar frame, canvas {side}, drawn {width} wide, {triangles} triangles in a layer: \
         mean {mean:.2} ms, median {median:.2}, p95 {p95:.2}, worst {worst:.2}",
        side = spec.side,
        mean = mean(&all),
        median = all[all.len() / 2],
        p95 = all[all.len() * 95 / 100],
        worst = all[all.len() - 1],
    );
    all
}

#[test]
#[ignore = "a timing, for a release build on a quiet machine"]
fn frame_budget_at_the_usual_size() {
    let spec = Spec {
        side: USUAL,
        ring: 64,
    };
    let mean = mean(&timings(spec, USUAL));
    assert!(
        mean < BUDGET_MS,
        "{mean:.2} ms is over the {BUDGET_MS:.2} ms budget"
    );
}

#[test]
#[ignore = "a timing, for a release build on a quiet machine"]
fn frame_cost_by_size() {
    for (side, width) in [
        (USUAL, USUAL / 2),
        (2 * USUAL, USUAL),
        (2 * USUAL, 2 * USUAL),
    ] {
        timings(Spec { side, ring: 64 }, width);
    }
}

/// Where the time of a frame of the usual size goes: the picture composed from the base, the eyes
/// and the mouth; the rule evaluated on the grid; the read of the picture through it.
#[test]
#[ignore = "a timing, for a release build on a quiet machine"]
fn frame_cost_by_step() {
    let spec = Spec {
        side: USUAL,
        ring: 64,
    };
    let (avatar, mut raster, frames) = setup(spec);
    let package = avatar.package();
    let canvas = (package.canvas.width, package.canvas.height);
    let mut field = Field::default();
    let mut out = vec![0; (USUAL * USUAL * 4) as usize];
    let steps: [(&str, Vec<f64>); 3] = [
        (
            "compose",
            times(&frames, |f| raster.compose(f, USUAL, USUAL)),
        ),
        (
            "grid of the rule",
            times(&frames, |f| {
                field.fill(&package.warp, &f.acting.pose, canvas, (USUAL, USUAL));
            }),
        ),
        (
            "read through the grid",
            times(&frames, |_| {
                resample(&field, avatar.base(), (USUAL, USUAL), &mut out);
            }),
        ),
    ];
    for (name, all) in steps {
        println!("{name}: {:.2} ms", mean(&all));
    }
}
