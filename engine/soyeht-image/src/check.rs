//! The deterministic check of one edited frame against the picture it came from: the same size,
//! a change that is not nothing and not most of the picture, and (when the agent says where the
//! feature is) a change that stays inside it. The measures are the frame spec's data.
//!
//! The picture is cut into squares; a square counts as changed when enough of its pixels moved
//! by more than a small amount, so the faint noise of a model that redraws the whole picture is
//! not a change. What the check cannot see (identity, framing, whether the mouth looks right)
//! the agent judges by looking at the picture; face landmarks arrive with the build (slice 9b).

use crate::{
    error::{Error, Result},
    frames::{Frame, Spec},
};
use png::{ColorType, Decoder, Transformations};
use serde::Serialize;
use std::{fs::File, io::BufReader, path::Path};

/// The longest side of a picture that is read; an avatar's picture is about a thousand pixels.
const MAX_SIDE: u32 = 2048;

/// A picture as 8-bit RGBA.
pub struct Picture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Picture {
    pub fn read(path: &Path) -> Result<Self> {
        let unreadable = |_| Error::Picture("not a PNG that can be read");
        let mut decoder = Decoder::new(BufReader::new(File::open(path)?));
        decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(unreadable)?;
        let (width, height) = (reader.info().width, reader.info().height);
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(Error::Picture("empty or larger than 2048 pixels on a side"));
        }
        let mut buffer = vec![0; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut buffer).map_err(unreadable)?;
        let channels = match frame.color_type {
            ColorType::Grayscale => 1,
            ColorType::GrayscaleAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
            ColorType::Indexed => {
                return Err(Error::Picture("an indexed picture was not expanded"));
            }
        };
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in buffer.chunks(frame.line_size).take(height as usize) {
            for pixel in row.chunks(channels).take(width as usize) {
                let (red, green, blue, alpha) = match *pixel {
                    [gray] => (gray, gray, gray, 255),
                    [gray, alpha] => (gray, gray, gray, alpha),
                    [red, green, blue] => (red, green, blue, 255),
                    [red, green, blue, alpha] => (red, green, blue, alpha),
                    _ => return Err(Error::Picture("a pixel with an unexpected size")),
                };
                rgba.extend_from_slice(&[red, green, blue, alpha]);
            }
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// A rectangle of the picture, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// The picture is not the size of the source.
    SizeDiffers,
    /// Nothing changed that matters.
    Unchanged,
    /// More of the picture changed than the frame allows.
    ChangedTooMuch,
    /// A change lies outside the region the agent gave.
    OutsideRegion,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub frame: String,
    pub ok: bool,
    pub problems: Vec<Problem>,
    pub width: u32,
    pub height: u32,
    /// The share of the picture's area covered by changed squares.
    pub changed_share: f64,
    pub min_change: f64,
    pub max_change: f64,
    /// The box around every changed square, to reuse as the region of the next frames.
    pub changed_box: Option<Rect>,
    /// Changed squares that lie outside the region (zero when no region was given).
    pub outside_squares: u32,
}

/// Checks `candidate` as the frame `frame` of `source`.
pub fn check(
    spec: &Spec,
    frame: &Frame,
    source: &Picture,
    candidate: &Picture,
    region: Option<Rect>,
) -> Result<Report> {
    let group = spec.group(frame)?;
    let mut report = Report {
        frame: frame.id.clone(),
        ok: false,
        problems: Vec::new(),
        width: candidate.width,
        height: candidate.height,
        changed_share: 0.0,
        min_change: group.min_change,
        max_change: group.max_change,
        changed_box: None,
        outside_squares: 0,
    };
    if source.size() != candidate.size() {
        report.problems.push(Problem::SizeDiffers);
        return Ok(report);
    }
    let squares = changed_squares(spec, source, candidate);
    report.changed_share = squares.share;
    report.changed_box = squares.bounds;
    if let Some(region) = region {
        let margin = spec.check.block;
        report.outside_squares = squares
            .rects
            .iter()
            .filter(|square| !inside(square, &region, margin))
            .count() as u32;
    }
    if squares.share < group.min_change {
        report.problems.push(Problem::Unchanged);
    }
    if squares.share > group.max_change {
        report.problems.push(Problem::ChangedTooMuch);
    }
    if report.outside_squares > 0 {
        report.problems.push(Problem::OutsideRegion);
    }
    report.ok = report.problems.is_empty();
    Ok(report)
}

fn inside(square: &Rect, region: &Rect, margin: u32) -> bool {
    square.x + margin >= region.x
        && square.y + margin >= region.y
        && square.x + square.width <= region.x + region.width + margin
        && square.y + square.height <= region.y + region.height + margin
}

struct Squares {
    rects: Vec<Rect>,
    share: f64,
    bounds: Option<Rect>,
}

fn changed_squares(spec: &Spec, source: &Picture, candidate: &Picture) -> Squares {
    let measure = &spec.check;
    let side = measure.block.max(1);
    let delta = i16::from(measure.pixel_delta);
    let mut rects = Vec::new();
    let mut area = 0u64;
    let mut y = 0;
    while y < source.height {
        let height = side.min(source.height - y);
        let mut x = 0;
        while x < source.width {
            let width = side.min(source.width - x);
            let mut moved = 0u32;
            for row in y..y + height {
                let start = (row * source.width + x) as usize * 4;
                let end = start + width as usize * 4;
                let (before, _) = source.rgba[start..end].as_chunks::<4>();
                let (after, _) = candidate.rgba[start..end].as_chunks::<4>();
                for (before, after) in before.iter().zip(after) {
                    let changed = before
                        .iter()
                        .zip(after)
                        .any(|(a, b)| (i16::from(*a) - i16::from(*b)).abs() > delta);
                    moved += u32::from(changed);
                }
            }
            if f64::from(moved) >= measure.active_share * f64::from(width * height) {
                area += u64::from(width) * u64::from(height);
                rects.push(Rect {
                    x,
                    y,
                    width,
                    height,
                });
            }
            x += side;
        }
        y += side;
    }
    let share = area as f64 / (f64::from(source.width) * f64::from(source.height));
    Squares {
        bounds: bounds(&rects),
        rects,
        share,
    }
}

fn bounds(rects: &[Rect]) -> Option<Rect> {
    let left = rects.iter().map(|rect| rect.x).min()?;
    let top = rects.iter().map(|rect| rect.y).min()?;
    let right = rects.iter().map(|rect| rect.x + rect.width).max()?;
    let bottom = rects.iter().map(|rect| rect.y + rect.height).max()?;
    Some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}
