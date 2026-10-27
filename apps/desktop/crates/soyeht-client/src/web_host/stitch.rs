//! Joining the screens of a read into one picture. Each later screen adds only the rows below
//! what the screens before it cover; the rules and the cases are the Mac client's
//! (`WebReadStitch`, `contracts/vectors/web-read.json`).

use ::image::{
    RgbImage,
    imageops::{self, FilterType},
};

/// One picture of the page and where on the page its first row is, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub offset: f64,
    pub height: f64,
}

/// The rows of one tile that the joined picture keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    pub tile: usize,
    pub source_y: f64,
    pub destination_y: f64,
    pub height: f64,
}

/// Each tile contributes only the rows below what the tiles before it already cover. The top of a
/// later tile is the part that repeats a pinned header, so it is dropped.
pub fn strips(tiles: &[Tile]) -> (Vec<Strip>, f64) {
    let Some(first) = tiles.first() else {
        return (Vec::new(), 0.0);
    };
    let mut covered = first.offset;
    let mut kept = Vec::new();
    for (index, tile) in tiles.iter().enumerate() {
        let bottom = tile.offset + tile.height;
        let start = covered.max(tile.offset);
        if bottom <= start {
            continue;
        }
        kept.push(Strip {
            tile: index,
            source_y: start - tile.offset,
            destination_y: start - first.offset,
            height: bottom - start,
        });
        covered = bottom;
    }
    (kept, covered - first.offset)
}

/// Pixels per logical pixel of the joined picture: the screen's own density, lowered so that the
/// whole page stays inside the pixel budget.
pub fn output_scale(width: f64, height: f64, native_scale: f64, pixel_budget: f64) -> f64 {
    if width <= 0.0 || height <= 0.0 {
        return native_scale;
    }
    native_scale.min((pixel_budget / (width * height)).sqrt())
}

/// The joined picture of `images` (each already cut to its tile's rows, `view_width` logical
/// pixels wide). The picture is never allocated at more than the budget.
pub fn join(
    images: &[RgbImage],
    tiles: &[Tile],
    view_width: f64,
    pixel_budget: f64,
) -> Option<RgbImage> {
    let (strips, height) = strips(tiles);
    let first = images.first()?;
    if view_width <= 0.0 || height <= 0.0 {
        return None;
    }
    let native = f64::from(first.width()) / view_width;
    let scale = output_scale(view_width, height, native, pixel_budget);
    let rows = |logical: f64| (logical * scale).round();
    // A size of the joined picture is a positive number of pixels well inside `u32`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pixels = |value: f64| value.max(1.0) as u32;
    let (wide, high) = (pixels(rows(view_width)), pixels(rows(height)));
    let mut canvas = RgbImage::from_pixel(wide, high, ::image::Rgb([255, 255, 255]));
    for strip in strips {
        let source = images.get(strip.tile)?;
        let source_native = f64::from(source.width()) / view_width;
        // Rows of the source, rounded, and kept inside it.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (crop_top, crop_rows) = {
            let top = (strip.source_y * source_native).round().max(0.0) as u32;
            let rows = (strip.height * source_native).round().max(1.0) as u32;
            (top.min(source.height().saturating_sub(1)), rows)
        };
        let crop_rows = crop_rows.min(source.height() - crop_top);
        let piece = imageops::crop_imm(source, 0, crop_top, source.width(), crop_rows).to_image();
        let top = rows(strip.destination_y);
        let bottom = rows(strip.destination_y + strip.height);
        let target = pixels(bottom - top);
        let piece = if piece.width() == wide && piece.height() == target {
            piece
        } else {
            imageops::resize(&piece, wide, target, FilterType::Triangle)
        };
        // The row the strip starts at: a non-negative whole number of pixels.
        #[allow(clippy::cast_possible_truncation)]
        let at = top.max(0.0) as i64;
        imageops::replace(&mut canvas, &piece, 0, at);
    }
    Some(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::vector;

    fn number(value: &serde_json::Value, name: &str) -> f64 {
        value[name].as_f64().unwrap()
    }

    #[test]
    fn the_vector_cases_of_the_stitch() {
        for case in vector("web-read")["stitch"].as_array().unwrap() {
            let tiles: Vec<Tile> = case["tiles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tile| Tile {
                    offset: number(tile, "offset"),
                    height: number(tile, "height"),
                })
                .collect();
            let (kept, height) = strips(&tiles);
            let name = &case["name"];
            assert_eq!(height, number(case, "height"), "{name}");
            let expected = case["strips"].as_array().unwrap();
            assert_eq!(kept.len(), expected.len(), "{name}");
            for (strip, expect) in kept.iter().zip(expected) {
                assert_eq!(
                    strip.tile as u64,
                    expect["tile"].as_u64().unwrap(),
                    "{name}"
                );
                assert_eq!(strip.source_y, number(expect, "sourceY"), "{name}");
                assert_eq!(
                    strip.destination_y,
                    number(expect, "destinationY"),
                    "{name}"
                );
                assert_eq!(strip.height, number(expect, "height"), "{name}");
            }
        }
    }

    #[test]
    fn the_vector_cases_of_the_output_scale() {
        for case in vector("web-read")["outputScale"].as_array().unwrap() {
            let scale = output_scale(
                number(case, "width"),
                number(case, "height"),
                number(case, "nativeScale"),
                number(case, "pixelBudget"),
            );
            assert!((scale - number(case, "scale")).abs() < 1e-9, "{scale}");
        }
    }

    /// A tile of `rows` rows, every row of the tone `tone(row)`.
    fn tile(rows: u32, tone: impl Fn(u32) -> u8) -> RgbImage {
        RgbImage::from_fn(10, rows, |_, row| ::image::Rgb([tone(row), 0, 0]))
    }

    #[test]
    fn every_row_of_the_page_is_at_its_own_place_in_the_joined_picture() {
        // A page of 300 rows whose tone is the row; three screens of 120 rows, 20 overlapping.
        let screen = |top: u32| tile(120, move |row| ((top + row) % 250) as u8);
        let images = [screen(0), screen(100), screen(200)];
        let tiles = [
            Tile {
                offset: 0.0,
                height: 120.0,
            },
            Tile {
                offset: 100.0,
                height: 120.0,
            },
            Tile {
                offset: 200.0,
                height: 120.0,
            },
        ];
        let joined = join(&images, &tiles, 10.0, 4_000_000.0).unwrap();
        assert_eq!((joined.width(), joined.height()), (10, 320));
        for row in [0_u32, 99, 100, 150, 219, 220, 319] {
            assert_eq!(joined.get_pixel(3, row)[0], (row % 250) as u8, "row {row}");
        }
    }

    #[test]
    fn a_joined_picture_over_the_budget_is_scaled_down_and_never_built_at_full_size() {
        let images = [tile(2_000, |_| 7)];
        let tiles = [Tile {
            offset: 0.0,
            height: 2_000.0,
        }];
        let joined = join(&images, &tiles, 10.0, 5_000.0).unwrap();
        assert!(u64::from(joined.width()) * u64::from(joined.height()) <= 5_100);
    }
}
