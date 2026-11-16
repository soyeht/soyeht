#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_image::{Error, Picture, Rect, check, spec};
use support::{Scratch, rgba_png, write};

const SIDE: u32 = 96;

fn base(x: u32, y: u32) -> [u8; 4] {
    let shade = ((x + y) % 200) as u8;
    [shade, shade, 90, 255]
}

/// The picture with a bright patch of 32 by 16 pixels at (32, 48).
fn with_patch(x: u32, y: u32) -> [u8; 4] {
    if (32..64).contains(&x) && (48..64).contains(&y) {
        [250, 20, 20, 255]
    } else {
        base(x, y)
    }
}

fn picture(scratch: &Scratch, name: &str, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Picture {
    let path = scratch.path(name);
    write(&path, &rgba_png(SIDE, SIDE, pixel));
    Picture::read(&path).unwrap()
}

fn report(
    frame: &str,
    candidate: impl Fn(u32, u32) -> [u8; 4],
    region: Option<Rect>,
) -> soyeht_image::Report {
    let scratch = Scratch::new();
    let spec = spec().unwrap();
    let source = picture(&scratch, "source.png", base);
    let candidate = picture(&scratch, "candidate.png", candidate);
    check(
        &spec,
        spec.frame(frame).unwrap(),
        &source,
        &candidate,
        region,
    )
    .unwrap()
}

#[test]
fn a_change_of_the_right_size_inside_the_region_passes_and_names_its_box() {
    let region = Rect {
        x: 24,
        y: 40,
        width: 48,
        height: 32,
    };
    let report = report("A", with_patch, Some(region));
    assert!(report.ok, "{:?}", report.problems);
    let patch = report.changed_box.unwrap();
    assert_eq!(
        (patch.x, patch.y, patch.width, patch.height),
        (32, 48, 32, 16)
    );
    assert!((report.changed_share - (32.0 * 16.0) / 9216.0).abs() < 1e-9);
    assert_eq!(report.outside_squares, 0);
}

#[test]
fn a_picture_that_did_not_change_fails() {
    let report = report("A", base, None);
    assert!(!report.ok);
    assert_eq!(format!("{:?}", report.problems), "[Unchanged]");
    assert!(report.changed_box.is_none());
}

#[test]
fn the_faint_noise_of_a_model_that_redraws_everything_is_not_a_change() {
    let report = report(
        "A",
        |x, y| {
            let [r, g, b, a] = base(x, y);
            [r.saturating_add(9), g.saturating_add(9), b, a]
        },
        None,
    );
    assert_eq!(format!("{:?}", report.problems), "[Unchanged]");
}

#[test]
fn a_picture_that_changed_everywhere_fails() {
    let report = report(
        "A",
        |x, y| {
            let [r, g, b, a] = base(x, y);
            [255 - r, 255 - g, 255 - b, a]
        },
        None,
    );
    assert_eq!(format!("{:?}", report.problems), "[ChangedTooMuch]");
}

#[test]
fn a_change_outside_the_region_fails_and_counts_the_squares() {
    let region = Rect {
        x: 0,
        y: 0,
        width: 24,
        height: 24,
    };
    let report = report("A", with_patch, Some(region));
    assert_eq!(format!("{:?}", report.problems), "[OutsideRegion]");
    assert_eq!(report.outside_squares, 2);
}

#[test]
fn another_size_fails_before_any_comparison() {
    let scratch = Scratch::new();
    let spec = spec().unwrap();
    let source = picture(&scratch, "source.png", base);
    let path = scratch.path("small.png");
    write(&path, &rgba_png(48, 48, base));
    let small = Picture::read(&path).unwrap();
    let report = check(&spec, spec.frame("A").unwrap(), &source, &small, None).unwrap();
    assert_eq!(format!("{:?}", report.problems), "[SizeDiffers]");
}

#[test]
fn a_file_that_is_not_a_usable_png_is_refused_with_a_reason() {
    let scratch = Scratch::new();
    let path = scratch.path("text.png");
    write(&path, b"not a picture");
    assert!(matches!(Picture::read(&path), Err(Error::Picture(_))));
    assert!(matches!(
        Picture::read(&scratch.path("missing.png")),
        Err(Error::Disk(_))
    ));
    let wide = scratch.path("wide.png");
    write(&wide, &rgba_png(2049, 1, base));
    assert!(matches!(Picture::read(&wide), Err(Error::Picture(_))));
}

#[test]
fn a_grayscale_picture_reads_like_any_other() {
    let scratch = Scratch::new();
    let path = scratch.path("gray.png");
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 16, 8);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&[100; 16 * 8])
        .unwrap();
    write(&path, &out);
    assert_eq!(Picture::read(&path).unwrap().size(), (16, 8));
}
