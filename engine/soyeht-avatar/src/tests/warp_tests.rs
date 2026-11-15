use serde_json::json;

use super::fixtures::package_json;
use crate::acting::Pose;
use crate::package::Warp;

fn warp() -> Warp {
    serde_json::from_value(package_json()["warp"].clone()).expect("a warp")
}

fn pose(f: impl FnOnce(&mut Pose)) -> Pose {
    let mut p = Pose {
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
    f(&mut p);
    p
}

#[test]
fn the_rest_pose_reads_every_point_from_itself() {
    let w = warp();
    for (x, y) in [
        (10.0, 10.0),
        (100.0, 89.0),
        (100.0, 106.0),
        (100.0, 170.0),
        (190.0, 190.0),
    ] {
        let [sx, sy] = w.source_point(&pose(|_| {}), x, y);
        assert!((sx - x).abs() < 1e-9 && (sy - y).abs() < 1e-9);
    }
}

#[test]
fn the_head_turns_about_the_neck_and_the_collar_stays() {
    let w = warp();
    let turned = pose(|p| p.rot = 6.0);
    // The neck does not move; a point in the hair does; a point on the chest does not.
    let neck = w.source_point(&turned, w.pivot[0], w.pivot[1]);
    assert!((neck[0] - w.pivot[0]).abs() < 1e-9 && (neck[1] - w.pivot[1]).abs() < 1e-9);
    let hair = w.source_point(&turned, 100.0, 30.0);
    assert!((hair[0] - 100.0).abs() > 1.0);
    let chest = w.source_point(&turned, 100.0, 180.0);
    assert!((chest[0] - 100.0).abs() < 1e-9 && (chest[1] - 180.0).abs() < 1e-9);
}

#[test]
fn a_brow_lifts_where_the_package_puts_it() {
    let mut w = warp();
    let lifted = pose(|p| p.brow_l = 3.0);
    let at = w.brows[0].center;
    // The picture is read from below: the brow moves up.
    assert!(w.source_point(&lifted, at[0], at[1])[1] > at[1] + 2.0);
    // The same face with the brow somewhere else: the lift follows it.
    w.brows[0].center = [60.0, 40.0];
    assert!(w.source_point(&lifted, 60.0, 40.0)[1] > 40.0 + 2.0);
    assert!((w.source_point(&lifted, at[0], at[1])[1] - at[1]).abs() < 0.5);
}

#[test]
fn the_chest_breathes_below_its_rows_and_the_jaw_drops() {
    let w = warp();
    let breathing = pose(|p| p.breath = 1.0);
    assert!((w.source_point(&breathing, 100.0, 180.0)[1] - 180.0 - w.chest.rise).abs() < 1e-9);
    assert!((w.source_point(&breathing, 100.0, 60.0)[1] - 60.0).abs() < 1e-9);
    let jaw = pose(|p| p.jaw = 2.0);
    let c = w.jaw.center;
    // The chin drops: the picture is read from above.
    assert!((w.source_point(&jaw, c[0], c[1])[1] - c[1] + 2.0).abs() < 1e-9);
}

#[test]
fn the_package_warp_is_data_a_pose_can_be_applied_to() {
    let value = package_json()["warp"].clone();
    assert_eq!(value["browTiltSpan"], json!(14));
    assert!(serde_json::from_value::<Warp>(value).is_ok());
}
