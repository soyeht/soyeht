//! A small avatar made of flat colours, for the tests: every picture is one colour, so a pixel of
//! a frame says which picture it was read from. The lips and the inner mouth are rings of
//! `ring` vertices around the mouth's centre, sized by the drawing's anchor in the parameter
//! space; the proportions are the canvas's, so one description gives a 96 pixel avatar for the
//! tests and a 512 pixel one for the frame budget.

use crate::avatar::{Avatar, LoadError, Pixmap};
use serde_json::{Value, json};
use soyeht_avatar::{AssetInfo, EYE_IDS, Package, Shape};
use std::f64::consts::TAU;

#[derive(Debug, Clone, Copy)]
pub(super) struct Spec {
    /// The canvas is a square this many pixels across.
    pub side: u32,
    /// Vertices of each contour of the mouth.
    pub ring: usize,
}

pub(super) const BASE: [u8; 4] = [90, 100, 110, 255];
pub(super) const MARKER: [u8; 4] = [250, 250, 20, 255];

/// The colour of a drawing of the mouth's atlas.
pub(super) fn shape_colour(shape: Shape) -> [u8; 4] {
    let at = Shape::ALL.iter().position(|s| *s == shape).unwrap_or(0) as u8;
    [200 - 18 * at, 40 + 22 * at, 60 + 15 * at, 255]
}

/// The colour of an eye frame.
pub(super) fn eye_colour(id: &str) -> [u8; 4] {
    let at = EYE_IDS.iter().position(|e| *e == id).unwrap_or(0) as u8;
    [20 + 28 * at, 230 - 20 * at, 150, 255]
}

impl Spec {
    fn scaled(self, fraction: f64) -> f64 {
        fraction * f64::from(self.side)
    }

    fn pair(self, x: f64, y: f64) -> [f64; 2] {
        [self.scaled(x), self.scaled(y)]
    }

    /// The crop of the canvas the mouth's drawings live in: `x, y, width, height`.
    pub(super) fn crop(self) -> [u32; 4] {
        let [x, y] = [self.scaled(0.35), self.scaled(0.52)];
        [
            x as u32,
            y as u32,
            self.scaled(0.3) as u32,
            self.scaled(0.2) as u32,
        ]
    }

    pub(super) fn mouth_centre(self) -> [f64; 2] {
        self.pair(0.5, 0.62)
    }

    /// The marker square in the base picture: `x, y, side`, inside the head's full influence.
    pub(super) fn marker(self) -> [u32; 3] {
        [
            self.scaled(0.25) as u32,
            self.scaled(0.28) as u32,
            (self.side / 20).max(2),
        ]
    }

    pub(super) fn eye_size(self) -> (u32, u32) {
        ((self.side / 16).max(2), (self.side / 32).max(2))
    }

    pub(super) fn eye_rect(self, index: usize) -> [u32; 4] {
        let (w, h) = self.eye_size();
        let x = self.scaled(0.2 + 0.15 * (index % 4) as f64) as u32;
        let y = self.scaled(0.36 + 0.06 * (index / 4) as f64) as u32;
        [x, y, w, h]
    }

    fn atlas_size(self) -> (u32, u32) {
        let crop = self.crop();
        (3 * crop[2], 3 * crop[3])
    }

    fn ellipse(self, half_width: f64, half_height: f64) -> Vec<[f64; 2]> {
        let [cx, cy] = self.mouth_centre();
        (0..self.ring)
            .map(|k| {
                let angle = TAU * k as f64 / self.ring as f64;
                [
                    cx + half_width * angle.cos(),
                    cy + half_height * angle.sin(),
                ]
            })
            .collect()
    }

    /// The inner and outer lip contours of a drawing, from its place in the parameter space.
    fn contours(self, shape: Shape) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
        let q = shape.anchor();
        let width = self.scaled(0.07 + 0.05 * q.w - 0.02 * q.r);
        let opening = self.scaled(0.004 + 0.05 * q.o * (1.0 - q.c));
        let lip = self.scaled(0.012 + 0.01 * q.r);
        (
            self.ellipse(width, opening),
            self.ellipse(width + lip, opening + lip),
        )
    }

    /// The edge of the area the drawings cover, inside the crop.
    fn boundary(self) -> Vec<[f64; 2]> {
        let crop = self.crop();
        self.ellipse(f64::from(crop[2]) * 0.49, f64::from(crop[3]) * 0.49)
    }

    /// Where a point of the canvas is in the atlas, for a drawing's cell.
    fn uv(self, shape: Shape, point: [f64; 2]) -> [f64; 2] {
        let cell = Shape::ALL.iter().position(|s| *s == shape).unwrap_or(0);
        let (column, row) = ((cell % 3) as f64, (cell / 3) as f64);
        let crop = self.crop().map(f64::from);
        [
            (column * crop[2] + point[0] - crop[0]) / (3.0 * crop[2]),
            (row * crop[3] + point[1] - crop[1]) / (3.0 * crop[3]),
        ]
    }

    /// One part: the contours of each drawing (`rings` gives them, in the order of the vertices),
    /// the triangles, and which drawings have a picture of their own for it.
    fn part(
        self,
        id: &str,
        rings: &dyn Fn(Shape) -> Vec<[f64; 2]>,
        triangles: Vec<usize>,
        has_picture: &dyn Fn(Shape) -> bool,
    ) -> Value {
        let shapes: Vec<Value> = Shape::ALL
            .iter()
            .map(|&shape| {
                let points = rings(shape);
                let mut entry = json!({
                    "id": shape.id(),
                    "positions": points.iter().flatten().collect::<Vec<_>>(),
                });
                if has_picture(shape) {
                    let uvs: Vec<f64> = points.iter().flat_map(|p| self.uv(shape, *p)).collect();
                    entry["uvs"] = json!(uvs);
                }
                entry
            })
            .collect();
        json!({ "id": id, "triangles": triangles, "shapes": shapes })
    }

    /// The document of the package.
    pub(super) fn document(self) -> Value {
        let n = self.ring;
        let strip: Vec<usize> = (0..n)
            .flat_map(|k| {
                let next = (k + 1) % n;
                [k, next, n + k, next, n + next, n + k]
            })
            .collect();
        let fan: Vec<usize> = (0..n).flat_map(|k| [k, (k + 1) % n, n]).collect();
        let outer = |s: Shape| [self.contours(s).1, self.boundary()].concat();
        let lips = |s: Shape| {
            let (inner, outer) = self.contours(s);
            [outer, inner].concat()
        };
        let open = |s: Shape| s.anchor().o > 0.0;
        // The inner mouth has a picture only where the mouth is open, and a sliver of an area
        // where it is not, as the lab's model has.
        let inner = |s: Shape| {
            let mut ring = self.contours(s).0;
            ring.push(self.mouth_centre());
            ring
        };
        let mut skin = self.part("skin", &outer, strip.clone(), &|_| true);
        // The skin fades out towards the edge of the area.
        skin["alphas"] = json!([vec![1.0; n], vec![0.0; n]].concat());
        let mut inside = self.part("inner", &inner, fan, &open);
        inside["hideWhenEmpty"] = json!(true);
        let (atlas_w, atlas_h) = self.atlas_size();
        let (eye_w, eye_h) = self.eye_size();
        let mut files = vec![
            json!({ "path": "base.png", "width": self.side, "height": self.side }),
            json!({ "path": "mouth_atlas.png", "width": atlas_w, "height": atlas_h }),
        ];
        files.extend(EYE_IDS.iter().map(
            |id| json!({ "path": format!("eyes/{id}.png"), "width": eye_w, "height": eye_h }),
        ));
        let eyes: Vec<Value> = EYE_IDS
            .iter()
            .enumerate()
            .map(|(i, id)| json!({ "id": id, "file": format!("eyes/{id}.png"), "rect": self.eye_rect(i) }))
            .collect();
        json!({
            "format": "soyeht-avatar",
            "version": "0.1",
            "canvas": { "width": self.side, "height": self.side },
            "files": files,
            "base": "base.png",
            "mouth": {
                "atlas": { "file": "mouth_atlas.png", "columns": 3, "rows": 3 },
                "parts": [
                    skin,
                    self.part("lips", &lips, strip, &|_| true),
                    inside,
                ],
            },
            "eyes": eyes,
            "warp": {
                "pivot": self.pair(0.5, 0.64),
                "zoomCenter": self.pair(0.5, 0.55),
                "head": { "center": self.pair(0.5, 0.38), "radii": self.pair(0.36, 0.38), "inner": 0.74 },
                "headFade": self.pair(0.585, 0.715),
                "face": { "center": self.pair(0.5, 0.445), "radii": self.pair(0.2, 0.21), "inner": 0.55 },
                "brows": [
                    { "center": self.pair(0.42, 0.31), "radii": self.pair(0.075, 0.025), "inner": 0.45 },
                    { "center": self.pair(0.575, 0.305), "radii": self.pair(0.075, 0.025), "inner": 0.45 },
                ],
                "browTiltSpan": self.scaled(0.07),
                "jaw": { "center": self.pair(0.5, 0.53), "radii": self.pair(0.145, 0.095), "inner": 0.5 },
                "cheek": { "center": self.pair(0.5, 0.46), "radii": self.pair(0.185, 0.055), "inner": 0.3 },
                "chest": { "from": self.scaled(0.625), "to": self.scaled(0.78), "centerX": self.scaled(0.5), "spread": 0.006, "rise": self.scaled(0.0035) },
            },
        })
    }

    /// The PNG files the document lists, as the loader would see them.
    pub(super) fn assets(self) -> Vec<AssetInfo> {
        let (atlas_w, atlas_h) = self.atlas_size();
        let (eye_w, eye_h) = self.eye_size();
        let mut out = vec![
            AssetInfo {
                path: "base.png".into(),
                bytes: 900,
                size: Some((self.side, self.side)),
            },
            AssetInfo {
                path: "mouth_atlas.png".into(),
                bytes: 700,
                size: Some((atlas_w, atlas_h)),
            },
        ];
        out.extend(EYE_IDS.iter().map(|id| AssetInfo {
            path: format!("eyes/{id}.png"),
            bytes: 120,
            size: Some((eye_w, eye_h)),
        }));
        out
    }

    /// The straight RGBA bytes of a listed picture: its width, height and texels.
    pub(super) fn picture_bytes(self, path: &str) -> Option<(u32, u32, Vec<u8>)> {
        let flat = |(w, h): (u32, u32), colour: &dyn Fn(u32, u32) -> [u8; 4]| {
            let bytes = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .flat_map(|(x, y)| colour(x, y))
                .collect();
            Some((w, h, bytes))
        };
        if path == "base.png" {
            let [mx, my, m] = self.marker();
            return flat((self.side, self.side), &|x, y| {
                if (mx..mx + m).contains(&x) && (my..my + m).contains(&y) {
                    MARKER
                } else {
                    BASE
                }
            });
        }
        if path == "mouth_atlas.png" {
            let crop = self.crop();
            return flat(self.atlas_size(), &|x, y| {
                let cell = (y / crop[3]) * 3 + x / crop[2];
                shape_colour(Shape::ALL[cell as usize % 9])
            });
        }
        let id = path.strip_prefix("eyes/")?.strip_suffix(".png")?;
        flat(self.eye_size(), &|_, _| eye_colour(id))
    }

    pub(super) fn picture(self, path: &str) -> Result<Pixmap, LoadError> {
        let (w, h, bytes) = self.picture_bytes(path).ok_or(LoadError::Picture)?;
        Pixmap::from_straight_rgba(w, h, &bytes).ok_or(LoadError::Picture)
    }

    pub(super) fn package(self) -> Package {
        Package::load(&self.document().to_string(), &self.assets()).expect("a valid package")
    }

    pub(super) fn avatar(self) -> Avatar {
        Avatar::from_pictures(self.package(), |path| self.picture(path)).expect("an avatar")
    }
}
