use super::synthetic::{BASE, MARKER, Spec, eye_colour, shape_colour};
use crate::avatar::{Picture, Rasteriser, resample::Field};
use soyeht_avatar::{
    ActingFrame, Audio, EYE_IDS, Eye, Frame, Gaze, Lid, MorphWeights, Params, Pose, Runtime, Shape,
    State, Tune, weights_from_params,
};
use std::sync::Arc;

pub(super) const SPEC: Spec = Spec {
    side: 160,
    ring: 32,
};

pub(super) const NEUTRAL: Pose = Pose {
    rot: 0.0,
    head_x: 0.0,
    head_y: 0.0,
    face_x: 0.0,
    face_y: 0.0,
    brow_l: 0.0,
    brow_r: 0.0,
    brow_tilt: 0.0,
    jaw: 0.0,
    cheek: 0.0,
    breath: 0.0,
    zoom: 1.0,
};

pub(super) const OPEN_EYES: Eye = Eye {
    gaze: Gaze::Center,
    lid: Lid::Open,
    soft: false,
};

pub(super) fn frame(weights: MorphWeights, eye: Eye, pose: Pose) -> Frame {
    Frame {
        mouth: Params::REST,
        weights,
        acting: ActingFrame {
            pose,
            eye,
            state: State::Idle,
        },
    }
}

fn rest() -> MorphWeights {
    weights_from_params(Params::REST)
}

/// A mouth that is `share` of the way from the resting drawing to the drawing `A`.
fn towards_a(share: f64) -> MorphWeights {
    let mut visemes = [0.0; 8];
    visemes[1] = share;
    MorphWeights { visemes }
}

/// The colour of a pixel as the textures are written: red, green, blue, alpha.
fn rgba(picture: &Picture, x: u32, y: u32) -> [u8; 4] {
    let at = (y * picture.width + x) as usize * 4;
    let b = &picture.bgra[at..at + 4];
    [b[2], b[1], b[0], b[3]]
}

fn near(a: [u8; 4], b: [u8; 4], by: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= by)
}

/// How many pixels of a column are this colour.
fn column_count(picture: &Picture, x: u32, colour: [u8; 4]) -> usize {
    (0..picture.height)
        .filter(|y| rgba(picture, x, *y) == colour)
        .count()
}

fn rasteriser(spec: Spec) -> Rasteriser {
    Rasteriser::new(Arc::new(spec.avatar()))
}

#[test]
fn a_resting_frame_is_the_base_picture_with_the_resting_mouth() {
    let mut raster = rasteriser(SPEC);
    let picture = raster.render(&frame(rest(), OPEN_EYES, NEUTRAL), SPEC.side);
    assert_eq!((picture.width, picture.height), (SPEC.side, SPEC.side));
    assert_eq!(rgba(&picture, 3, 3), BASE);
    let [mx, my, m] = SPEC.marker();
    assert_eq!(rgba(&picture, mx + m / 2, my + m / 2), MARKER);
    // Just above the centre of the mouth are the lips of the resting drawing.
    let [cx, cy] = SPEC.mouth_centre();
    assert_eq!(
        rgba(&picture, cx as u32, cy as u32 - 2),
        shape_colour(Shape::Rest)
    );
}

#[test]
fn the_mouth_is_one_geometry_never_two_half_visible_mouths() {
    let mut raster = rasteriser(SPEC);
    let [cx, _] = SPEC.mouth_centre();
    let x = cx as u32;
    let inside = shape_colour(Shape::A);
    let open = column_count(
        &raster.render(&frame(towards_a(1.0), OPEN_EYES, NEUTRAL), SPEC.side),
        x,
        inside,
    );
    let half = raster.render(&frame(towards_a(0.5), OPEN_EYES, NEUTRAL), SPEC.side);
    let aperture = column_count(&half, x, inside);
    // Half way the aperture is half of the open one, filled with the one inner mouth there is.
    assert!(open >= 12, "{open}");
    assert!(aperture.abs_diff(open / 2) <= 2, "{aperture} of {open}");
    // The lips are the weighted average of the two drawings, at the one half-open geometry.
    let [_, cy] = SPEC.mouth_centre();
    let lip = rgba(&half, x, (cy - aperture as f64 / 2.0 - 1.0) as u32);
    let mean = |c: usize| {
        let (a, b) = (shape_colour(Shape::Rest)[c], shape_colour(Shape::A)[c]);
        ((u16::from(a) + u16::from(b)) / 2) as u8
    };
    assert!(near(lip, [mean(0), mean(1), mean(2), 255], 2), "{lip:?}");
}

#[test]
fn the_eye_frame_follows_the_look() {
    let mut raster = rasteriser(SPEC);
    let looks = [
        (Gaze::Center, Lid::Closed, false, Some("blink")),
        (Gaze::Left, Lid::Half, false, Some("half")),
        (Gaze::Left, Lid::Open, false, Some("left")),
        (Gaze::Right, Lid::Open, false, Some("right")),
        (Gaze::Up, Lid::Open, false, Some("up")),
        (Gaze::Down, Lid::Open, false, Some("down")),
        (Gaze::UpLeft, Lid::Open, false, Some("upleft")),
        (Gaze::Center, Lid::Open, true, Some("soft")),
        (Gaze::Left, Lid::Open, true, Some("left")),
        (Gaze::Center, Lid::Open, false, None),
    ];
    for (gaze, lid, soft, wanted) in looks {
        let eye = Eye { gaze, lid, soft };
        let picture = raster.render(&frame(rest(), eye, NEUTRAL), SPEC.side);
        for (i, id) in EYE_IDS.iter().enumerate() {
            let [x, y, w, h] = SPEC.eye_rect(i);
            let seen = rgba(&picture, x + w / 2, y + h / 2);
            let expected = if wanted == Some(*id) {
                eye_colour(id)
            } else {
                BASE
            };
            assert_eq!(seen, expected, "{gaze:?} {lid:?} {soft}: frame {id}");
        }
    }
}

#[test]
fn the_head_moves_with_the_pose() {
    let mut raster = rasteriser(SPEC);
    let [mx, my, m] = SPEC.marker();
    let pose = Pose {
        head_x: 6.0,
        ..NEUTRAL
    };
    let picture = raster.render(&frame(rest(), OPEN_EYES, pose), SPEC.side);
    let y = my + m / 2;
    assert_eq!(rgba(&picture, mx + 5, y), BASE);
    assert_eq!(rgba(&picture, mx + 6, y), MARKER);
    assert_eq!(rgba(&picture, mx + m + 5, y), MARKER);
    assert_eq!(rgba(&picture, mx + m + 6, y), BASE);
}

#[test]
fn a_smaller_output_shows_the_same_picture() {
    let mut raster = rasteriser(SPEC);
    let picture = raster.render(&frame(rest(), OPEN_EYES, NEUTRAL), SPEC.side / 2);
    assert_eq!(
        (picture.width, picture.height),
        (SPEC.side / 2, SPEC.side / 2)
    );
    let [mx, my, m] = SPEC.marker();
    assert_eq!(rgba(&picture, mx / 2 + m / 4, my / 2 + m / 4), MARKER);
    assert_eq!(rgba(&picture, 1, 1), BASE);
    assert_eq!(raster.output_size(0), (1, 1));
}

#[test]
fn the_same_frame_draws_the_same_pixels() {
    let wanted = frame(
        towards_a(0.3),
        OPEN_EYES,
        Pose {
            rot: 2.0,
            jaw: 3.0,
            ..NEUTRAL
        },
    );
    let first = rasteriser(SPEC).render(&wanted, 100).bgra.to_vec();
    let mut raster = rasteriser(SPEC);
    raster.render(&frame(rest(), OPEN_EYES, NEUTRAL), 100);
    assert_eq!(raster.render(&wanted, 100).bgra, first);
}

#[test]
fn the_grid_reads_where_the_rule_says() {
    let package = Spec { side: 512, ring: 8 }.package();
    let pose = Pose {
        rot: 3.0,
        head_x: 4.0,
        head_y: -2.0,
        face_x: 2.0,
        face_y: 1.0,
        brow_l: 3.0,
        brow_r: 2.0,
        brow_tilt: 1.0,
        jaw: 4.0,
        cheek: 2.0,
        breath: 1.2,
        zoom: 1.05,
    };
    let canvas = (package.canvas.width, package.canvas.height);
    let out = (512, 400);
    let mut field = Field::default();
    field.fill(&package.warp, &pose, canvas, out);
    let mut worst = 0.0_f64;
    for y in (0..out.1).step_by(3) {
        for x in (0..out.0).step_by(2) {
            let (cx, cy) = (
                (f64::from(x) + 0.5) * 512.0 / f64::from(out.0),
                (f64::from(y) + 0.5) * 512.0 / f64::from(out.1),
            );
            let exact = package.warp.source_point(&pose, cx, cy);
            let (read, _, _) = field.row_run(x, y);
            worst = worst
                .max((exact[0] - f64::from(read[0])).abs())
                .max((exact[1] - f64::from(read[1])).abs());
        }
    }
    // The strongest pose the acting layer makes, on a face drawn 512 across: a fifth of a pixel.
    assert!(worst < 0.2, "the grid is {worst} pixels off the rule");
}

#[test]
fn a_spoken_clip_animates_the_avatar_and_every_frame_is_whole() {
    let avatar = Arc::new(SPEC.avatar());
    let mut raster = Rasteriser::new(Arc::clone(&avatar));
    let mut runtime = Runtime::new(avatar.package(), Tune::default(), 7);
    let mut distinct = std::collections::HashSet::new();
    for i in 0..180 {
        let t = f64::from(i) / 60.0;
        let db = -40.0 + 22.0 * (0.5 + 0.5 * (t * 22.0).sin());
        let audio = Some(Audio {
            position: t,
            db: Some(db),
        });
        let wanted = runtime.step(1.0 / 60.0, State::Speaking, audio);
        let picture = raster.render(&wanted, SPEC.side);
        assert_eq!(picture.bgra.len(), (SPEC.side * SPEC.side * 4) as usize);
        assert!(picture.bgra.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        distinct.insert(picture.bgra.to_vec());
    }
    assert!(distinct.len() > 60, "{} distinct frames", distinct.len());
}
