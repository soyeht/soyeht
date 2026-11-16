//! The software rasteriser: one frame of the runtime and the avatar's pictures become the pixels of
//! a picture, on one core.
//!
//! A frame is drawn in three steps, each by a rule of `soyeht-avatar` and none of them a choice
//! between pictures of a mouth:
//!
//! 1. the base picture, with the eye frame the acting layer asks for laid over it;
//! 2. the mouth: the three parts of the mesh (skin, lips, inner mouth, in the order
//!    `MouthModel::in_drawing_order` gives), at the vertices `MeshPart::blend` gives. Each part is
//!    drawn once: every drawing `MeshPart::layers` names is read at the pixel and mixed with the
//!    others (the exact weighted average), and that picture is laid over what is below at the
//!    opacity of the part (`MeshPart::opacity`) and the feather of its corners (`composite` of
//!    the crate is the rule), so the edge of a part does not move when a drawing comes in or
//!    goes out;
//! 3. the pose: the composed picture read through `Warp::source_point` (`resample`).
//!
//! Drawing in software keeps one algorithm on every renderer of the avatar (the Mac and iOS draw
//! the same triangles into a `CGImage`), and the work is bounded: the mesh is a few thousand
//! triangles, small on the picture, and the last pass costs by the pixels of the output.

use super::{
    art::Avatar,
    pixmap::Pixmap,
    resample::{Field, resample},
    triangles::{Corner, Reading, fill},
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

/// One part of the mouth, at the geometry of the weights: each triangle is drawn once, reading the
/// cell of every drawing that takes part, and the part is laid over the picture once. The
/// drawings are mixed with each other, never one by one with what is below, so a drawing coming
/// in or out changes the colour of the part and not the edge of it.
fn draw_part(target: &mut Pixmap, atlas: &Pixmap, part: &MeshPart, weights: &[f64; 9]) {
    let drawn: Vec<(&[f64], f32)> = part
        .layers(weights)
        .iter()
        .filter_map(|layer| {
            let uvs = part
                .shapes
                .iter()
                .find(|s| s.id == layer.shape.id())?
                .uvs
                .as_deref()?;
            Some((uvs, layer.alpha as f32))
        })
        .collect();
    // How much of the part is drawn: the inner mouth comes in with the weights of the drawings
    // that have a picture of it, not whole when one passes the threshold.
    let opacity = part.opacity(weights) as f32;
    if drawn.is_empty() || opacity <= 0.0 {
        return;
    }
    let positions = part.blend(weights);
    for triangle in part.triangles.as_chunks::<3>().0 {
        let corners = triangle.map(|index| {
            let i = index as usize;
            let at = positions.get(2 * i..2 * i + 2)?;
            let feather = part
                .alphas
                .as_ref()
                .and_then(|a| a.get(i))
                .map_or(1.0, |a| *a as f32);
            Some(Corner {
                at: [at[0] as f32, at[1] as f32],
                feather: feather * opacity,
            })
        });
        let [Some(a), Some(b), Some(c)] = corners else {
            continue;
        };
        let readings: Vec<Reading> = drawn
            .iter()
            .filter_map(|(uvs, alpha)| {
                let texel = triangle.map(|index| {
                    let i = index as usize;
                    uvs.get(2 * i..2 * i + 2)
                        .map(|uv| [uv[0] as f32, uv[1] as f32])
                });
                let [Some(a), Some(b), Some(c)] = texel else {
                    return None;
                };
                Some(Reading {
                    texel: [a, b, c],
                    alpha: *alpha,
                })
            })
            .collect();
        fill(target, atlas, [a, b, c], &readings);
    }
}
