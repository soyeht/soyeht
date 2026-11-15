//! The avatar package as data: one JSON document (`avatar.json`) next to the PNG files it names.
//!
//! Our own format (the owner's decision of 2026-11-04, ADR 0166); Live2D, Spine and Inochi2D are
//! references only. The package holds the base picture, the mouth model (nine drawings' lip
//! contours with one shared topology, in three parts: skin, lips, inner mouth), the eye frames
//! with their rectangles, the warp (where the pose applies on this face) and the acting
//! constants, which include the mouth's resting expressions as parameters, not pictures. The
//! format is unstable (0.x, see `VERSION`): a reader accepts one version exactly. Unknown fields
//! are ignored.

use serde::{Deserialize, Serialize};

use crate::acting_tune::ActingTune;

/// The `format` every package carries.
pub const FORMAT: &str = "soyeht-avatar";
/// The version this crate reads, and the only one. The format is **unstable** (0.x): it has not
/// yet been consumed by a renderer (slices 4 and 5 of ADR 0166) or written by the build command
/// (slice 9b), and any of them may change it. Until then a new minor version may break an old
/// one, so a reader accepts exactly this string. The first stable version will be 1.0, and from
/// then on a later minor version only adds fields.
pub const VERSION: &str = "0.1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub format: String,
    /// Exactly `VERSION`.
    pub version: String,
    /// The size of the avatar's picture in pixels; every rectangle and position is in it.
    pub canvas: Size,
    /// Every PNG of the package. A file the document names must be listed here.
    pub files: Vec<FileDecl>,
    /// The base picture (the rest state: open eyes, resting mouth).
    pub base: String,
    pub mouth: MouthModel,
    /// The seven eye frames and the blink frame, by id (see `EYE_IDS`). The resting expressions of
    /// the mouth (a listening smile, a thinking "hm") are not pictures: they are targets of the
    /// continuous mouth, in the acting constants (`ActingTune::expression`).
    pub eyes: Vec<Patch>,
    /// Where on this face the pose is applied.
    pub warp: Warp,
    #[serde(default)]
    pub acting: ActingTune,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDecl {
    /// A relative path with `/` separators, inside the package folder.
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// An edited region of the base picture: the PNG holds just the rectangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patch {
    pub id: String,
    pub file: String,
    /// `x, y, width, height` in the canvas.
    pub rect: [u32; 4],
}

/// A soft ellipse of influence on the canvas: full strength inside `inner` (a share of the
/// radii) and falling smoothly to nothing at the radii.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// `x, y` of the centre, in canvas pixels.
    pub center: [f64; 2],
    /// Horizontal and vertical radius, in canvas pixels.
    pub radii: [f64; 2],
    /// The share of the radii (0 to under 1) inside which the influence is full.
    pub inner: f64,
}

/// Where the pose of the acting layer applies on this face: the places a displacement of the
/// picture is centred on. They belong to the picture (a neck is where it is), so they are in the
/// package, in canvas pixels; `Warp::source_point` is the one rule that uses them, and the
/// renderers share it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Warp {
    /// The neck: the head rotates about this point.
    pub pivot: [f64; 2],
    /// The picture's zoom is centred here.
    pub zoom_center: [f64; 2],
    /// The head (hair, face, ears), which moves and rotates as one.
    pub head: Region,
    /// Rows of the picture, `from` to `to`, over which the head's influence fades out (the neck:
    /// the collar stays put).
    pub head_fade: [f64; 2],
    /// The face features (eyes, nose, mouth), which move a little more than the head's outline.
    pub face: Region,
    /// The left and right brow, each lifted as a soft blob.
    pub brows: [Region; 2],
    /// The distance along the brow over which its inner end lifts more than the outer one.
    pub brow_tilt_span: f64,
    /// The lower face, which drops with the jaw.
    pub jaw: Region,
    /// The cheeks, which lift with a smile.
    pub cheek: Region,
    pub chest: Chest,
}

/// The chest and shoulders, which breathe: below `from` the picture rises by `rise` pixels at the
/// full breath and widens by `spread` of the distance from `center_x`, fully at `to`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chest {
    pub from: f64,
    pub to: f64,
    pub center_x: f64,
    pub spread: f64,
    pub rise: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouthModel {
    /// The picture of the nine drawings' mouth crops, `columns` by `rows` cells, one per shape in
    /// the order of `Shape::ALL`.
    pub atlas: Atlas,
    /// `skin`, `lips` and `inner`, once each.
    pub parts: Vec<MeshPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Atlas {
    pub file: String,
    pub columns: u32,
    pub rows: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PartKind {
    /// The skin around the lips.
    Skin,
    /// The ring of the lips.
    Lips,
    /// Teeth and tongue, inside the inner contour; drawn only while a drawing with an aperture
    /// takes part.
    Inner,
}

impl PartKind {
    pub const ALL: [PartKind; 3] = [PartKind::Skin, PartKind::Lips, PartKind::Inner];
}

/// One part of the mouth mesh: nine shapes with the same vertex list and one triangulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshPart {
    pub id: PartKind,
    /// Vertex indices, three per triangle.
    pub triangles: Vec<u32>,
    /// The opacity of each vertex (the feather at the edge of the skin); all 1 when absent.
    #[serde(default)]
    pub alphas: Option<Vec<f64>>,
    /// Above 1 sharpens the weights of the pictures (the nearest drawings dominate), never the
    /// geometry's.
    #[serde(default = "one")]
    pub texture_power: f64,
    #[serde(default)]
    pub hide_when_empty: bool,
    /// One entry per drawing, in any order, each id once.
    pub shapes: Vec<ShapeMesh>,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeMesh {
    /// `rest, closed_MBP, A, E, I, O, U, FV, LDT`.
    pub id: String,
    /// `x, y` pairs on screen, in the canvas's pixels.
    pub positions: Vec<f64>,
    /// `u, v` pairs in the atlas (0..=1) inside this shape's cell. Absent for a drawing with no
    /// picture of its own for this part (an inner mouth with the lips closed).
    #[serde(default)]
    pub uvs: Option<Vec<f64>>,
}

/// The eye frames every package carries.
pub const EYE_IDS: [&str; 8] = [
    "half", "left", "right", "up", "down", "upleft", "soft", "blink",
];
