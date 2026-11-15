use super::fixtures::{assets, package_json};
use super::mouth_limits::package;
use crate::package::{ErrorKind, Package, PartKind, png_size};
use crate::params::{Params, Shape, weights_from_params};

#[test]
fn the_synthetic_package_loads() {
    let p = Package::load(&package_json().to_string(), &assets()).expect("valid");
    assert_eq!(p.mouth.parts.len(), 3);
    assert_eq!(p.acting, crate::acting_tune::ActingTune::default());
}

#[test]
fn a_field_a_later_minor_version_adds_is_ignored() {
    let mut doc = package_json();
    doc["version"] = "1.4".into();
    doc["futureThing"] = serde_json::json!({ "x": 1 });
    doc["mouth"]["parts"][0]["newField"] = 3.into();
    assert!(Package::load(&doc.to_string(), &assets()).is_ok());
}

#[test]
fn acting_constants_a_package_leaves_out_keep_their_default() {
    let mut doc = package_json();
    doc["acting"] = serde_json::json!({ "blink": { "doubleChance": 0.5 } });
    let p = Package::load(&doc.to_string(), &assets()).expect("valid");
    assert_eq!(p.acting.blink.double_chance, 0.5);
    assert_eq!(
        p.acting.blink.gap,
        crate::acting_tune::BlinkTune::default().gap
    );
}

#[test]
fn a_failure_names_where_and_never_the_value() {
    let mut doc = package_json();
    doc["mouth"]["parts"][1]["triangles"][4] = 9999.into();
    let err = Package::load(&doc.to_string(), &assets()).expect_err("index");
    assert_eq!(err.kind, ErrorKind::Index);
    assert_eq!(err.at, "/mouth/parts/1/triangles");
    assert!(!err.at.contains("9999"));
}

#[test]
fn png_size_reads_the_header() {
    let mut png = vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
    ];
    png.extend(640u32.to_be_bytes());
    png.extend(480u32.to_be_bytes());
    assert_eq!(png_size(&png), Some((640, 480)));
    assert_eq!(png_size(&png[..20]), None);
    png[1] = b'X';
    assert_eq!(png_size(&png), None);
}

#[test]
fn the_inner_mouth_is_drawn_only_while_a_drawing_with_an_aperture_takes_part() {
    let p = package();
    let inner = p
        .mouth
        .parts
        .iter()
        .find(|part| part.id == PartKind::Inner)
        .expect("inner");
    let skin = p
        .mouth
        .parts
        .iter()
        .find(|part| part.id == PartKind::Skin)
        .expect("skin");
    let closed = weights_from_params(Shape::Rest.anchor()).all();
    assert!(inner.layers(&closed).is_empty());
    assert_eq!(
        skin.layers(&closed).first().map(|l| l.shape),
        Some(Shape::Rest)
    );
    let open = weights_from_params(Params::new(1.0, 0.55, 0.1, 0.0, 0.0)).all();
    assert!(inner.layers(&open).iter().any(|l| l.shape == Shape::A));
}

#[test]
fn the_nearest_drawings_dominate_the_picture_of_the_skin() {
    let p = package();
    let skin = p
        .mouth
        .parts
        .iter()
        .find(|part| part.id == PartKind::Skin)
        .expect("skin");
    assert!(skin.texture_power >= 1.0);
    let mut w = [0.0; 9];
    w[Shape::A.index()] = 0.7;
    w[Shape::E.index()] = 0.3;
    let layers = skin.layers(&w);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].alpha, 1.0);
}
