//! The last pass of a frame: the composed picture, read through the pose.
//!
//! `Warp::source_point` (the rule every renderer shares) says where in the picture each point
//! drawn is read from. It is evaluated at the corners of a grid of 4 pixel cells, and each pixel
//! inside a cell interpolates the four corners' answers: the displacement is smooth (a
//! smoothstep of an ellipse, a linear breath), so over the large places of a face it is read to a
//! small fraction of a pixel with about four thousand evaluations in place of a quarter of a
//! million. A linear read of a smoothstep is off by about the square of the cell over the size of
//! the place, so the cells that lie on a small place (a brow, a cheek) are made finer, down to 2
//! pixels, and only those.

use super::pixmap::Pixmap;
use soyeht_avatar::{Pose, Region, Warp};

/// The side of a cell where the pose moves only large places, in pixels of the output.
const COARSE: u32 = 4;
const COARSE_SHIFT: u32 = 2;
/// The side of a cell on a small place.
const FINE: u32 = 2;
/// A cell is this many times smaller than the place it lies on, so a place is read by several
/// cells across (a linear read of a smoothstep is off by about the square of that ratio).
const CELLS_ACROSS: f64 = 8.0;

/// What a cell of the grid holds: where its four corners read from, or, for a cell that lies on
/// a small place (a brow), the corners of finer cells.
#[derive(Default)]
pub(super) struct Field {
    /// Coarse cells across and down.
    cells: (usize, usize),
    /// The side of the fine cells of each coarse cell (`COARSE` when it has none), row by row.
    sides: Vec<u32>,
    /// Where the corners of each coarse cell start in `corners`, row by row.
    starts: Vec<usize>,
    /// The corners, cell by cell: the four of a coarse cell, or the grid of a refined one.
    corners: Vec<[f32; 2]>,
}

/// A place the pose moves, in pixels of the output: its box and the cell side it needs.
struct Place {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    side: u32,
}

/// The places of the warp as they lie on the canvas: the rule reads each place at the point the
/// zoom brings to the pixel, so a zoomed pose has its places moved and grown by it.
fn places(warp: &Warp, zoom: f64, scale: (f64, f64)) -> Vec<Place> {
    let regions: [&Region; 6] = [
        &warp.head,
        &warp.face,
        &warp.brows[0],
        &warp.brows[1],
        &warp.jaw,
        &warp.cheek,
    ];
    regions
        .iter()
        .map(|r| {
            let [zx, zy] = warp.zoom_center;
            let (cx, cy) = (
                (r.center[0] - zx) * zoom + zx,
                (r.center[1] - zy) * zoom + zy,
            );
            let (rx, ry) = (r.radii[0] * zoom * scale.0, r.radii[1] * zoom * scale.1);
            // A radius that is not a number (the validator refuses one) needs no finer cell.
            let wanted = (rx.min(ry) / CELLS_ACROSS).floor();
            let wanted = if wanted.is_finite() {
                wanted
            } else {
                f64::from(COARSE)
            };
            Place {
                left: cx * scale.0 - rx,
                top: cy * scale.1 - ry,
                right: cx * scale.0 + rx,
                bottom: cy * scale.1 + ry,
                side: if wanted < f64::from(COARSE) {
                    FINE
                } else {
                    COARSE
                },
            }
        })
        .collect()
}

impl Field {
    /// Evaluates the rule for an output of `out` pixels that shows a picture of `canvas`.
    pub(super) fn fill(&mut self, warp: &Warp, pose: &Pose, canvas: (u32, u32), out: (u32, u32)) {
        let scale = (
            f64::from(out.0) / f64::from(canvas.0),
            f64::from(out.1) / f64::from(canvas.1),
        );
        let places = places(warp, pose.zoom.max(1e-6), scale);
        let at = |x: u32, y: u32| {
            let [sx, sy] = warp.source_point(pose, f64::from(x) / scale.0, f64::from(y) / scale.1);
            // The rule answers in pixels of the canvas; the picture read is at the size of the output.
            [(sx * scale.0) as f32, (sy * scale.1) as f32]
        };
        let cells = (
            out.0.div_ceil(COARSE) as usize,
            out.1.div_ceil(COARSE) as usize,
        );
        self.cells = cells;
        self.sides.clear();
        self.starts.clear();
        self.corners.clear();
        for row in 0..cells.1 as u32 {
            for column in 0..cells.0 as u32 {
                let (left, top) = (column * COARSE, row * COARSE);
                let (right, bottom) = (left + COARSE, top + COARSE);
                let side = places
                    .iter()
                    .filter(|p| {
                        p.left < f64::from(right)
                            && p.right > f64::from(left)
                            && p.top < f64::from(bottom)
                            && p.bottom > f64::from(top)
                    })
                    .map(|p| p.side)
                    .min()
                    .unwrap_or(COARSE);
                self.sides.push(side);
                self.starts.push(self.corners.len());
                for y in (top..=bottom).step_by(side as usize) {
                    for x in (left..=right).step_by(side as usize) {
                        self.corners.push(at(x, y));
                    }
                }
            }
        }
    }

    /// Where the centre of output pixel (`x`, `y`) reads from, how that moves for each pixel
    /// along the row to its right, and how many pixels that holds for: a row of a cell is a
    /// straight line across it. The sides are powers of two, so this is shifts and masks.
    pub(super) fn row_run(&self, x: u32, y: u32) -> ([f32; 2], [f32; 2], u32) {
        let cell = (x >> COARSE_SHIFT) as usize + (y >> COARSE_SHIFT) as usize * self.cells.0;
        let shift = self.sides[cell].trailing_zeros();
        let across = ((COARSE >> shift) + 1) as usize;
        let (in_x, in_y) = (x & (COARSE - 1), y & (COARSE - 1));
        let first =
            self.starts[cell] + (in_y >> shift) as usize * across + (in_x >> shift) as usize;
        let [a, b] = [self.corners[first], self.corners[first + 1]];
        let [c, d] = [
            self.corners[first + across],
            self.corners[first + across + 1],
        ];
        let (offset_x, offset_y) = (in_x & ((1 << shift) - 1), in_y & ((1 << shift) - 1));
        let inverse = 1.0 / (1 << shift) as f32;
        let fx = (offset_x as f32 + 0.5) * inverse;
        let fy = (offset_y as f32 + 0.5) * inverse;
        let along = |k: usize| {
            let left = a[k] + (c[k] - a[k]) * fy;
            let right = b[k] + (d[k] - b[k]) * fy;
            (left + (right - left) * fx, (right - left) * inverse)
        };
        let (x_start, x_step) = along(0);
        let (y_start, y_step) = along(1);
        (
            [x_start, y_start],
            [x_step, y_step],
            (1 << shift) - offset_x,
        )
    }
}

/// A position in the composed picture in 65536ths of a pixel, with the centre of the first
/// texel at 0, kept far inside the range of the integer: a pose never moves a point that far.
fn fixed(v: f32) -> i32 {
    ((v - 0.5).clamp(-REACH, REACH) * 65536.0) as i32
}

/// The most a position may lie outside the picture, in pixels: the edge is repeated anyway.
const REACH: f32 = 16_000.0;

/// Writes the composed picture through `field` into `bgra`: `out` pixels, four bytes each, blue
/// first and not premultiplied, the order GPUI's images are in. The picture is `composed`,
/// whose size is the canvas's.
pub(super) fn resample(field: &Field, composed: &Pixmap, out: (u32, u32), bgra: &mut [u8]) {
    for (y, row) in bgra.chunks_exact_mut(out.0 as usize * 4).enumerate() {
        let mut x = 0;
        while x < out.0 {
            let (start, step, run) = field.row_run(x, y as u32);
            let run = run.min(out.0 - x);
            let pixels = &mut row[x as usize * 4..(x + run) as usize * 4];
            let (mut at_x, mut at_y) = (fixed(start[0]), fixed(start[1]));
            let (step_x, step_y) = ((step[0] * 65536.0) as i32, (step[1] * 65536.0) as i32);
            for pixel in pixels.as_chunks_mut::<4>().0 {
                let texel = composed.sample_fixed(at_x >> 8, at_y >> 8);
                *pixel = blue_first(texel).to_le_bytes();
                at_x = at_x.saturating_add(step_x);
                at_y = at_y.saturating_add(step_y);
            }
            x += run;
        }
    }
}

/// A premultiplied texel as the bytes blue, green, red, alpha of a picture that is not
/// premultiplied, packed so that blue is the lowest byte.
fn blue_first(texel: u32) -> u32 {
    let alpha = texel >> 24;
    let (red, green, blue) = (texel & 0xFF, (texel >> 8) & 0xFF, (texel >> 16) & 0xFF);
    match alpha {
        0 => 0,
        255 => blue | green << 8 | red << 16 | alpha << 24,
        a => {
            let lift = |c: u32| ((c * 255 + a / 2) / a).min(255);
            lift(blue) | lift(green) << 8 | lift(red) << 16 | a << 24
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixmap::pack;

    #[test]
    fn an_opaque_texel_swaps_red_and_blue_and_a_clear_one_is_nothing() {
        assert_eq!(
            blue_first(pack([10, 20, 30, 255])).to_le_bytes(),
            [30, 20, 10, 255]
        );
        assert_eq!(blue_first(pack([0, 0, 0, 0])), 0);
    }

    #[test]
    fn a_translucent_texel_is_made_straight_again() {
        assert_eq!(
            blue_first(pack([50, 25, 100, 128])).to_le_bytes(),
            [199, 50, 100, 128]
        );
    }
}
