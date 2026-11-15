//! The avatar package as data: one JSON document (`avatar.json`) next to the PNG files it names.
//!
//! Our own format (the owner's decision of 2026-11-04, ADR 0166); Live2D, Spine and Inochi2D are
//! references only. The package holds the base picture, the mouth model (nine drawings' lip
//! contours with one shared topology, in three parts: skin, lips, inner mouth), the eye and
//! resting-mouth frames with their rectangles, the warp lattice of the head and the acting
//! constants. Unknown fields are ignored, so a later minor version can add to it.

use serde::{Deserialize, Serialize};

use crate::acting_tune::ActingTune;

/// The `format` every package carries.
pub const FORMAT: &str = "soyeht-avatar";
/// The major version this crate reads. A change of the major version is a change of what a
/// package means; a minor version only adds.
pub const MAJOR_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub format: String,
    /// `major.minor`.
    pub version: String,
    /// The size of the avatar's picture in pixels; every rectangle and position is in it.
    pub canvas: Size,
    /// Every PNG of the package. A file the document names must be listed here.
    pub files: Vec<FileDecl>,
    /// The base picture (the rest state: open eyes, resting mouth).
    pub base: String,
    pub mouth: MouthModel,
    /// The seven eye frames and the blink frame, by id (see `EYE_IDS`).
    pub eyes: Vec<Patch>,
    /// The resting mouths of listening (`smile`) and thinking (`hm`).
    pub rest_mouths: Vec<Patch>,
    pub warp: Lattice,
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

/// The warp lattice of the head: a grid of `cols` by `rows` cells over `rect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lattice {
    pub rect: [u32; 4],
    pub cols: u32,
    pub rows: u32,
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
/// The resting mouths every package carries.
pub const REST_MOUTH_IDS: [&str; 2] = ["smile", "hm"];
