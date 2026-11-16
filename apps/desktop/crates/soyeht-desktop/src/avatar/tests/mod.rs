//! Tests of the avatar on the desktop: the rasteriser over a synthetic avatar of flat colours
//! (`synthetic`), the loader over a package folder, and the frame budget, which is measured by
//! hand (`cargo test --release -- --ignored --nocapture frame_budget`).

mod budget;
mod load_tests;
mod raster_tests;
mod synthetic;
