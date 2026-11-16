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
use soyeht_avatar::{Frame, MeshPart, eye_frame};
use std::sync::Arc;

/// A picture drawn, borrowed from the rasteriser until its next frame.
#[derive(Debug, Clone, Copy)]
pub struct Picture<'a> {
    pub width: u32,
    pub height: u32,
    /// Blue, green, red, alpha for each pixel, row by row, not premultiplied.
    pub bgra: &'a [u8],
}

/// The pictures of the avatar at the size of the picture drawn: the base, the eye frames and the
/// atlas of the mouth, each reduced once by an area average (see `Pixmap::downscaled`) so that
/// drawing from them is a read at about one texel for each pixel, however small the picture is.
struct Level {
    width: u32,
    base: Pixmap,
    atlas: Pixmap,
    /// The eye frames by id, with the rectangle each takes at this size.
    eyes: Vec<(String, [u32; 4], Pixmap)>,
    /// Pixels of this level for a pixel of the canvas, across and down.
    scale: (f32, f32),
}

impl Level {
    fn new(avatar: &Avatar, width: u32, height: u32) -> Self {
        let canvas = avatar.package().canvas;
        let scale = (
            width as f32 / canvas.width as f32,
            height as f32 / canvas.height as f32,
        );
        let reduced = |picture: &Pixmap| {
            let size = |side: u32, by: f32| ((side as f32 * by).round() as u32).max(1);
            picture.downscaled(
                size(picture.width(), scale.0),
                size(picture.height(), scale.1),
            )
        };
        let eyes = avatar
            .eyes()
            .iter()
            .map(|patch| {
                // The rectangle covers the pixels the patch touches, so it never leaves a seam.
                let [x, y, w, h] = patch.rect;
                let left = (x as f32 * scale.0).floor() as u32;
                let top = (y as f32 * scale.1).floor() as u32;
                let right = ((x + w) as f32 * scale.0).ceil() as u32;
                let bottom = ((y + h) as f32 * scale.1).ceil() as u32;
                let rect = [left, top, (right - left).max(1), (bottom - top).max(1)];
                (
                    patch.id.clone(),
                    rect,
                    patch.picture.downscaled(rect[2], rect[3]),
                )
            })
            .collect();
        Self {
            width,
            base: avatar.base().downscaled(width, height),
            atlas: reduced(avatar.atlas()),
            eyes,
            scale,
        }
    }
}

pub struct Rasteriser {
    avatar: Arc<Avatar>,
    /// The pictures at the size last drawn; made again when the size changes.
    level: Option<Level>,
    /// The base picture with this frame's eyes and mouth on it, at the size of the picture.
    composed: Pixmap,
    field: Field,
    out: Vec<u8>,
}

impl Rasteriser {
    pub fn new(avatar: Arc<Avatar>) -> Self {
        let composed = avatar.base().clone();
        Self {
            avatar,
            level: None,
            composed,
            field: Field::default(),
            out: Vec::new(),
        }
    }

    /// The size a picture of `width` pixels across has: the canvas's proportions, never larger
    /// than the canvas and at least one pixel each way.
    pub fn output_size(&self, width: u32) -> (u32, u32) {
        let canvas = self.avatar.package().canvas;
        let width = width.clamp(1, canvas.width);
        let height =
            (u64::from(width) * u64::from(canvas.height)).div_ceil(u64::from(canvas.width));
        (width, u32::try_from(height).unwrap_or(u32::MAX).max(1))
    }

    /// Draws `frame` as a picture `width` pixels across (see `output_size`).
    pub fn render(&mut self, frame: &Frame, width: u32) -> Picture<'_> {
        let (width, height) = self.output_size(width);
        self.compose(frame, width, height);
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

    /// Steps 1 and 2: the base picture, the eyes and the mouth, at the size of the picture.
    pub(super) fn compose(&mut self, frame: &Frame, width: u32, height: u32) {
        if self.level.as_ref().is_none_or(|level| level.width != width) {
            self.level = Some(Level::new(&self.avatar, width, height));
        }
        let Some(level) = &self.level else {
            return;
        };
        self.composed.copy_from(&level.base);
        let wanted = eye_frame(frame.acting.eye);
        if let Some((_, rect, picture)) = level
            .eyes
            .iter()
            .find(|(id, _, _)| Some(id.as_str()) == wanted)
        {
            lay_patch(&mut self.composed, picture, *rect);
        }
        let weights = frame.weights.all();
        for part in self.avatar.package().mouth.in_drawing_order() {
            draw_part(
                &mut self.composed,
                &level.atlas,
                level.scale,
                part,
                &weights,
            );
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

/// One part of the mouth, at the geometry of the weights (in pixels of the canvas, times `scale`): each triangle is drawn once, reading the
/// cell of every drawing that takes part, and the part is laid over the picture once. The
/// drawings are mixed with each other, never one by one with what is below, so a drawing coming
/// in or out changes the colour of the part and not the edge of it.
fn draw_part(
    target: &mut Pixmap,
    atlas: &Pixmap,
    scale: (f32, f32),
    part: &MeshPart,
    weights: &[f64; 9],
) {
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
                at: [at[0] as f32 * scale.0, at[1] as f32 * scale.1],
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
