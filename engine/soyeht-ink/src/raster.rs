//! Draws the strokes as the image a recognizer reads: dark round-capped lines on white, the
//! same on every system. The scale is chosen from the writing itself, because a client's units
//! say nothing about how big the letters are.

use crate::writing::{Dot, Writing};

/// The height of a small letter (a, c, e, o) in the image. A recognizer read this size better
/// than twice it (measured on real pen strokes; ADR 0158).
const X_HEIGHT_PX: f64 = 48.0;
/// The pen's width as a share of the small letter's height: 4 px at the size above.
const PEN_RATIO: f64 = 1.0 / 12.0;
/// The thinnest pen drawn, so a writing squeezed into the longest side stays visible.
const PEN_MIN_PX: f64 = 1.5;
/// White around the writing, in pixels.
const MARGIN_PX: f64 = 38.0;
/// The longest side of the image. A writing that is wider than this at the nominal size is
/// drawn smaller.
const LONGEST_SIDE_PX: f64 = 3000.0;
/// A stroke counts as a letter-sized stroke when it is at least this share of the tallest one.
/// Dots, accents and a stray tap are left out of the estimate of the letter's height.
const SIGNIFICANT_SHARE: f64 = 0.35;
/// A letter-sized stroke is taller than a small letter: it holds ascenders and descenders.
/// This turns the median height of such strokes into the height of a small letter.
const X_HEIGHT_SHARE: f64 = 0.75;
/// The smallest letter height, in the writing's units, that is scaled up to the image size. A
/// writing smaller than this (a coordinate range that is subnormal) is drawn as the same dot a
/// single point is, instead of a scale that overflows.
const MIN_LETTER: f64 = 1e-9;
const WHITE: u8 = 255;

/// An 8-bit grayscale image, row 0 at the top, 255 white and 0 ink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Bitmap {
    fn white(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![WHITE; width * height],
        }
    }

    /// Darkens a pixel to `coverage` (0 to 1) of ink; a pixel already darker stays.
    fn ink(&mut self, x: usize, y: usize, coverage: f64) {
        let level = (f64::from(WHITE) * (1.0 - coverage)).round() as u8;
        let pixel = &mut self.pixels[y * self.width + x];
        *pixel = (*pixel).min(level);
    }
}

struct Frame {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

fn frame(strokes: &[Vec<Dot>]) -> Frame {
    let (mut left, mut top) = (f64::INFINITY, f64::INFINITY);
    let (mut right, mut bottom) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for dot in strokes.iter().flatten() {
        left = left.min(dot.x);
        right = right.max(dot.x);
        top = top.min(dot.y);
        bottom = bottom.max(dot.y);
    }
    Frame {
        left,
        top,
        width: right - left,
        height: bottom - top,
    }
}

/// The height, in the writing's units, of a small letter.
fn x_height(strokes: &[Vec<Dot>], frame: &Frame) -> f64 {
    let mut heights = strokes
        .iter()
        .map(|stroke| {
            let (low, high) = stroke
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), dot| {
                    (low.min(dot.y), high.max(dot.y))
                });
            high - low
        })
        .collect::<Vec<_>>();
    let tallest = heights.iter().copied().fold(0.0, f64::max);
    heights.retain(|height| *height >= tallest * SIGNIFICANT_SHARE);
    heights.sort_by(f64::total_cmp);
    let median = heights.get(heights.len() / 2).copied().unwrap_or(0.0);
    let estimate = median * X_HEIGHT_SHARE;
    if estimate > 0.0 {
        return estimate;
    }
    // Only points and level lines: a dash or a tap. Size it by the writing's extent.
    frame.width.max(frame.height).max(f64::EPSILON)
}

/// Draws `writing`. The image is never larger than the longest side, and it is white where
/// nothing was written.
pub fn render(writing: &Writing) -> Bitmap {
    let frame = frame(&writing.strokes);
    let letter = x_height(&writing.strokes, &frame).max(MIN_LETTER);
    let mut scale = X_HEIGHT_PX / letter;
    let longest = frame.width.max(frame.height);
    let room = LONGEST_SIDE_PX - 2.0 * MARGIN_PX;
    if longest * scale > room {
        scale = room / longest;
    }
    let radius = (letter * scale * PEN_RATIO).max(PEN_MIN_PX) / 2.0;
    let width = (frame.width * scale + 2.0 * MARGIN_PX).ceil() as usize;
    let height = (frame.height * scale + 2.0 * MARGIN_PX).ceil() as usize;
    let mut bitmap = Bitmap::white(width, height);
    let place = |dot: &Dot| {
        (
            (dot.x - frame.left) * scale + MARGIN_PX,
            (dot.y - frame.top) * scale + MARGIN_PX,
        )
    };
    for stroke in &writing.strokes {
        let mut from = place(&stroke[0]);
        draw_segment(&mut bitmap, from, from, radius);
        for dot in &stroke[1..] {
            let to = place(dot);
            draw_segment(&mut bitmap, from, to, radius);
            from = to;
        }
    }
    bitmap
}

/// Draws the capsule of `radius` around the segment from `a` to `b`, edges anti-aliased by the
/// distance of each pixel centre to the segment.
fn draw_segment(bitmap: &mut Bitmap, a: (f64, f64), b: (f64, f64), radius: f64) {
    let reach = radius + 1.0;
    let first = (a.1.min(b.1) - reach).floor().max(0.0) as usize;
    let last = ((a.1.max(b.1) + reach).ceil().max(0.0) as usize).min(bitmap.height - 1);
    for row in first..=last {
        let centre = row as f64 + 0.5;
        let Some((from, to)) = clip_to_band(a, b, centre - reach, centre + reach) else {
            continue;
        };
        let start = (from.min(to) - reach).floor().max(0.0) as usize;
        let end = ((from.max(to) + reach).ceil().max(0.0) as usize).min(bitmap.width - 1);
        for column in start..=end {
            let distance = distance_to_segment((column as f64 + 0.5, centre), a, b);
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                bitmap.ink(column, row, coverage);
            }
        }
    }
}

/// The x range of the part of segment `a`-`b` that lies between the heights `low` and `high`.
fn clip_to_band(a: (f64, f64), b: (f64, f64), low: f64, high: f64) -> Option<(f64, f64)> {
    let rise = b.1 - a.1;
    if rise == 0.0 {
        return (low..=high).contains(&a.1).then_some((a.0, b.0));
    }
    let along = |y: f64| ((y - a.1) / rise).clamp(0.0, 1.0);
    let (near, far) = (along(low), along(high));
    let (start, end) = (near.min(far), near.max(far));
    let at = |t: f64| a.0 + (b.0 - a.0) * t;
    let (y_start, y_end) = (a.1 + rise * start, a.1 + rise * end);
    // Both ends outside the band on the same side: the segment misses it.
    let outside = (y_start < low && y_end < low) || (y_start > high && y_end > high);
    (!outside).then(|| (at(start), at(end)))
}

fn distance_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_squared).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + dx * t)).hypot(p.1 - (a.1 + dy * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::{InkPoint, InkRecognizeParams};

    fn writing(strokes: &[&[(f64, f64)]]) -> Writing {
        Writing::from_params(&InkRecognizeParams {
            strokes: strokes
                .iter()
                .map(|stroke| {
                    stroke
                        .iter()
                        .enumerate()
                        .map(|(index, (x, y))| InkPoint {
                            x: *x,
                            y: *y,
                            t: index as u32,
                        })
                        .collect()
                })
                .collect(),
            languages: vec!["en".into()],
            max_candidates: None,
        })
        .unwrap()
    }

    fn dark(bitmap: &Bitmap) -> usize {
        bitmap.pixels.iter().filter(|pixel| **pixel < 128).count()
    }

    fn at(bitmap: &Bitmap, x: usize, y: usize) -> u8 {
        bitmap.pixels[y * bitmap.width + x]
    }

    #[test]
    fn a_line_is_drawn_dark_on_white_with_the_margin_around_it() {
        let image = render(&writing(&[&[(0.0, 0.0), (100.0, 0.0)]]));
        // A level line has no height: it is sized by its length and the margin surrounds it.
        assert_eq!(
            image.width,
            (100.0 * (X_HEIGHT_PX / 100.0) + 2.0 * MARGIN_PX).ceil() as usize
        );
        let middle = image.height / 2;
        assert_eq!(
            at(&image, image.width / 2, middle.saturating_sub(1)).min(at(
                &image,
                image.width / 2,
                middle
            )),
            0
        );
        assert_eq!(at(&image, 2, 2), WHITE);
        assert_eq!(at(&image, image.width - 3, image.height - 3), WHITE);
    }

    #[test]
    fn the_same_writing_in_other_units_gives_the_same_image() {
        let small = render(&writing(&[
            &[(0.0, 0.0), (3.0, 12.0), (6.0, 0.0)],
            &[(1.0, 6.0), (5.0, 6.0)],
        ]));
        let large = render(&writing(&[
            &[(10.0, 100.0), (40.0, 220.0), (70.0, 100.0)],
            &[(20.0, 160.0), (60.0, 160.0)],
        ]));
        assert_eq!((small.width, small.height), (large.width, large.height));
        let differing = small
            .pixels
            .iter()
            .zip(&large.pixels)
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert!(
            differing < small.pixels.len() / 200,
            "{differing} pixels differ"
        );
    }

    #[test]
    fn the_letters_are_drawn_at_the_height_a_recognizer_reads_best() {
        // Five strokes of one height h: the small letter is estimated as 0.75 h.
        let strokes: Vec<Vec<(f64, f64)>> = (0..5)
            .map(|index| vec![(index as f64 * 10.0, 0.0), (index as f64 * 10.0, 8.0)])
            .collect();
        let refs = strokes.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let image = render(&writing(&refs));
        let letter = 8.0 * X_HEIGHT_SHARE;
        let scale = X_HEIGHT_PX / letter;
        assert_eq!(
            image.height,
            (8.0 * scale + 2.0 * MARGIN_PX).ceil() as usize
        );
    }

    #[test]
    fn dots_and_accents_do_not_shrink_the_estimate_of_the_letter() {
        let tall = [(0.0, 0.0), (0.0, 20.0)];
        let dot = [(5.0, 0.0), (5.0, 1.0)];
        let with_dots = render(&writing(&[&tall, &tall, &dot, &dot, &dot, &dot, &dot]));
        let without = render(&writing(&[&tall, &tall]));
        assert_eq!(with_dots.height, without.height);
    }

    #[test]
    fn a_very_long_writing_is_drawn_smaller_and_fits_the_longest_side() {
        let image = render(&writing(&[&[(0.0, 0.0), (1_000_000.0, 10.0)]]));
        assert!(image.width as f64 <= LONGEST_SIDE_PX + 1.0);
        assert!(dark(&image) > 0, "the pen stays visible");
    }

    #[test]
    fn one_point_makes_a_dot_and_does_not_divide_by_zero() {
        let image = render(&writing(&[&[(5.0, 5.0)]]));
        assert!(dark(&image) > 0);
        assert!(image.width >= (2.0 * MARGIN_PX) as usize);
        let far = render(&writing(&[&[(999_999.0, -999_999.0)]]));
        assert_eq!((image.width, image.height), (far.width, far.height));
    }

    #[test]
    fn a_writing_of_subnormal_extent_is_drawn_as_a_dot_and_does_not_overflow() {
        for extent in [1e-310, 5e-324, 1e-300, 1e-12] {
            for strokes in [
                vec![vec![(0.0, 0.0), (0.0, extent)]],
                vec![vec![(0.0, 0.0), (extent, 0.0)]],
                vec![
                    vec![(0.0, 0.0), (extent, extent)],
                    vec![(0.0, 0.0), (0.0, extent)],
                ],
            ] {
                let refs = strokes.iter().map(Vec::as_slice).collect::<Vec<_>>();
                let image = render(&writing(&refs));
                assert!(image.width >= (2.0 * MARGIN_PX) as usize, "{extent}");
                assert!(image.width as f64 <= LONGEST_SIDE_PX + 1.0, "{extent}");
                assert!(image.height as f64 <= LONGEST_SIDE_PX + 1.0, "{extent}");
                assert!(dark(&image) > 0, "the pen stays visible for {extent}");
            }
        }
    }

    #[test]
    fn the_edge_of_a_diagonal_stroke_is_anti_aliased() {
        let image = render(&writing(&[&[(0.0, 0.0), (10.0, 10.0)]]));
        let grays = image
            .pixels
            .iter()
            .filter(|pixel| (1..254).contains(*pixel))
            .count();
        assert!(grays > 20, "{grays} gray pixels");
    }

    #[test]
    fn a_stroke_reaches_the_corner_of_a_pixel_grid_without_leaving_it() {
        // Segments that run along and across the border of the writing's frame.
        let image = render(&writing(&[&[
            (0.0, 0.0),
            (50.0, 0.0),
            (50.0, 50.0),
            (0.0, 50.0),
            (0.0, 0.0),
        ]]));
        assert!(dark(&image) > 100);
        assert_eq!(image.pixels.len(), image.width * image.height);
    }
}
