//! The software rasteriser: one frame of the runtime and the avatar's pictures become the pixels of
//! a picture, on one core.
//!
//! A frame is drawn in three steps, each by a rule of `soyeht-avatar` and none of them a choice
//! between pictures of a mouth:
//!
//! 1. the base picture, with the eye frame the acting layer asks for laid over it;
//! 2. the mouth: the three parts of the mesh (skin, lips, inner mouth, in the order
//!    `MouthModel::in_drawing_order` gives) at the geometry the morph
//!    weights blend (`MeshPart::blend`), each textured by the drawings the weights name, laid
//!    one over the other with the opacities `MeshPart::layers` gives, which make the picture the
//!    exact weighted average of the drawings;
//! 3. the pose: the composed picture read through `Warp::source_point` (`resample`).
//!
//! Drawing in software keeps one algorithm on every renderer of the avatar (the Mac and iOS draw
//! the same triangles into a `CGImage`), and the work is bounded: the mesh is a few thousand
//! triangles, small on the picture, and the last pass costs by the pixels of the output.

use super::{
    art::Avatar,
    pixmap::Pixmap,
    resample::{Field, resample},
    triangles::{Corner, fill},
};
use soyeht_avatar::{Frame, MeshPart};
use std::sync::Arc;

/// A picture drawn, borrowed from the rasteriser until its next frame.
#[derive(Debug, Clone, Copy)]
pub struct Picture<'a> {
    pub width: u32,
    pub height: u32,
    /// Blue, green, red, alpha for each pixel, row by row, not premultiplied.
    pub bgra: &'a [u8],
}

pub struct Rasteriser {
    avatar: Arc<Avatar>,
    /// The base picture with this frame's eyes and mouth on it, in the pixels of the canvas.
    composed: Pixmap,
    field: Field,
    out: Vec<u8>,
}

impl Rasteriser {
    pub fn new(avatar: Arc<Avatar>) -> Self {
        let composed = avatar.base().clone();
        Self {
            avatar,
            composed,
            field: Field::default(),
            out: Vec::new(),
        }
    }

    /// The size a picture of `width` pixels across has: the canvas's proportions, at least one
    /// pixel each way.
    pub fn output_size(&self, width: u32) -> (u32, u32) {
        let canvas = self.avatar.package().canvas;
        let width = width.max(1);
        let height =
            (u64::from(width) * u64::from(canvas.height)).div_ceil(u64::from(canvas.width));
        (width, u32::try_from(height).unwrap_or(u32::MAX).max(1))
    }

    /// Draws `frame` as a picture `width` pixels across.
    pub fn render(&mut self, frame: &Frame, width: u32) -> Picture<'_> {
        let (width, height) = self.output_size(width);
        self.compose(frame);
        let package = self.avatar.package();
        let canvas = (package.canvas.width, package.canvas.height);
        self.field
            .fill(&package.warp, &frame.acting.pose, canvas, (width, height));
        self.out.resize(width as usize * height as usize * 4, 0);
        resample(&self.field, &self.composed, (width, height), &mut self.out);
        Picture {
            width,
            height,
            bgra: &self.out,
        }
    }

    /// Steps 1 and 2: the base picture, the eyes and the mouth, in the pixels of the canvas.
    pub(super) fn compose(&mut self, frame: &Frame) {
        let avatar = Arc::clone(&self.avatar);
        self.composed.copy_from(avatar.base());
        if let Some(patch) = avatar.eye_patch(frame.acting.eye) {
            lay_patch(&mut self.composed, &patch.picture, patch.rect);
        }
        let weights = frame.weights.all();
        for part in avatar.package().mouth.in_drawing_order() {
            draw_part(&mut self.composed, avatar.atlas(), part, &weights);
        }
    }
}

/// Lays an eye frame over the picture, at its place.
fn lay_patch(target: &mut Pixmap, patch: &Pixmap, rect: [u32; 4]) {
    for (row, texels) in patch.rows().enumerate() {
        for (column, texel) in texels.iter().enumerate() {
            target.lay_over(rect[0] + column as u32, rect[1] + row as u32, *texel, 256);
        }
    }
}

/// One part of the mouth, at the geometry of the weights: each layer of the picture is the part's
/// triangles, read from the cell of one drawing.
fn draw_part(target: &mut Pixmap, atlas: &Pixmap, part: &MeshPart, weights: &[f64; 9]) {
    let positions = part.blend(weights);
    for layer in part.layers(weights) {
        let Some(uvs) = part
            .shapes
            .iter()
            .find(|s| s.id == layer.shape.id())
            .and_then(|s| s.uvs.as_ref())
        else {
            continue;
        };
        let corner = |index: u32| {
            let i = index as usize;
            Some(Corner {
                at: [
                    *positions.get(2 * i)? as f32,
                    *positions.get(2 * i + 1)? as f32,
                ],
                texel: [*uvs.get(2 * i)? as f32, *uvs.get(2 * i + 1)? as f32],
                alpha: part
                    .alphas
                    .as_ref()
                    .and_then(|a| a.get(i))
                    .map_or(1.0, |a| *a as f32),
            })
        };
        for triangle in part.triangles.as_chunks::<3>().0 {
            if let (Some(a), Some(b), Some(c)) = (
                corner(triangle[0]),
                corner(triangle[1]),
                corner(triangle[2]),
            ) {
                fill(target, atlas, [a, b, c], layer.alpha as f32);
            }
        }
    }
}
