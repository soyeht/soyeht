//! A screenshot for the engine: the picture the web view took, as a PNG that keeps to the
//! contract's size, which is also the size the Mac client's screenshots keep to.

use super::{HostError, attachment};
use ::image::{
    ExtendedColorType, ImageEncoder, RgbImage,
    codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder},
    imageops::{self, FilterType},
};
use soyeht_protocol::{AttachmentRef, limits};

/// The most pixels a screenshot holds; a longer page is scaled down to it.
const PIXEL_BUDGET: u64 = 4_000_000;
/// One pixel in this many is compared when two pictures are asked whether they agree.
const SAMPLE_EVERY: usize = 97;
/// How many times a PNG that is still too large is scaled down before the host gives up.
const SHRINKS: u32 = 8;

/// What the web view drew: blue, green, red and alpha, in rows of `stride` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub bytes: Vec<u8>,
}

impl Picture {
    /// Whether the two pictures show the same page, give or take what moves by itself on it: a
    /// few of the sampled pixels may differ.
    pub(super) fn agrees_with(&self, other: &Self) -> bool {
        if self.width != other.width || self.height != other.height || self.stride != other.stride {
            return false;
        }
        let samples = self
            .bytes
            .chunks(4 * SAMPLE_EVERY)
            .zip(other.bytes.chunks(4 * SAMPLE_EVERY));
        let (mut total, mut differing) = (0_u32, 0_u32);
        for (a, b) in samples {
            total += 1;
            if a.get(..3) != b.get(..3) {
                differing += 1;
            }
        }
        // At most one sample in fifty may differ.
        differing * 50 <= total
    }

    pub(super) fn to_rgb(&self) -> Option<RgbImage> {
        let (Ok(width), Ok(height)) = (usize::try_from(self.width), usize::try_from(self.height))
        else {
            return None;
        };
        if width == 0 || height == 0 || self.stride < width.checked_mul(4)? {
            return None;
        }
        if self.bytes.len() < self.stride.checked_mul(height)? {
            return None;
        }
        let mut rgb = Vec::with_capacity(width * height * 3);
        for row in self.bytes.chunks_exact(self.stride).take(height) {
            for pixel in row[..width * 4].as_chunks::<4>().0 {
                rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
        }
        RgbImage::from_raw(self.width, self.height, rgb)
    }
}

/// The screenshot as the engine takes it: its reference and its bytes.
pub(super) fn screenshot(picture: &Picture) -> Result<(AttachmentRef, Vec<u8>), HostError> {
    let png = bounded_png(picture)?;
    Ok((attachment(&png), png))
}

fn bounded_png(picture: &Picture) -> Result<Vec<u8>, HostError> {
    let image = picture.to_rgb().ok_or(HostError::Internal(
        "The page snapshot could not be encoded.",
    ))?;
    png_within_budget(image).map(|(png, _)| png)
}

/// The picture as a PNG inside the contract's bytes and pixels: scaled down, and when that is not
/// enough cut to its top rows, which `true` says. A picture is never refused for its size alone.
pub(super) fn png_within_budget(mut image: RgbImage) -> Result<(Vec<u8>, bool), HostError> {
    let pixels = u64::from(image.width()) * u64::from(image.height());
    if pixels > PIXEL_BUDGET {
        // The scale that brings the picture to the budget keeps its proportions.
        #[allow(clippy::cast_precision_loss)]
        let scale = (PIXEL_BUDGET as f64 / pixels as f64).sqrt();
        image = scaled(&image, scale)?;
    }
    let mut cut = false;
    for round in 0..=SHRINKS * 2 {
        let png = encode(&image)?;
        if png.len() <= limits::WEB_AGENT_IMAGE_BYTES {
            return Ok((png, cut));
        }
        if round < SHRINKS {
            image = scaled(&image, 0.75)?;
        } else {
            let rows = (image.height() * 3 / 4).max(1);
            if rows == image.height() {
                break;
            }
            image = imageops::crop_imm(&image, 0, 0, image.width(), rows).to_image();
            cut = true;
        }
    }
    Err(HostError::Internal(
        "The page snapshot exceeds its byte limit.",
    ))
}

fn scaled(image: &RgbImage, scale: f64) -> Result<RgbImage, HostError> {
    let side = |extent: u32| {
        // A scaled side is a small positive number of pixels.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let scaled = (f64::from(extent) * scale).floor() as u32;
        scaled.max(1)
    };
    let (width, height) = (side(image.width()), side(image.height()));
    if width == image.width() && height == image.height() {
        return Err(HostError::Internal(
            "The page snapshot could not be resized.",
        ));
    }
    Ok(imageops::resize(image, width, height, FilterType::Triangle))
}

fn encode(image: &RgbImage) -> Result<Vec<u8>, HostError> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, PngFilter::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|_| HostError::Internal("The page snapshot could not be encoded."))?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page-like picture of `width` by `height`: stripes, so it compresses but is not flat.
    pub(super) fn picture(width: u32, height: u32) -> Picture {
        let stride = width as usize * 4;
        let mut bytes = Vec::with_capacity(stride * height as usize);
        for row in 0..height {
            for column in 0..width {
                let tone = ((row / 8 + column / 8) % 2 * 200) as u8;
                bytes.extend_from_slice(&[tone, 255 - tone, 90, 255]);
            }
        }
        Picture {
            width,
            height,
            stride,
            bytes,
        }
    }

    #[test]
    fn a_small_page_is_a_png_with_the_name_the_engine_requires() {
        let (reference, bytes) = screenshot(&picture(64, 48)).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(reference.mime_type, "image/png");
        assert_eq!(reference.name, "screenshot.png");
        assert_eq!(reference.bytes, bytes.len().to_string());
        assert_eq!(reference.attachment_id.len(), 36);
        let decoded = ::image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 48));
    }

    #[test]
    fn a_long_page_is_scaled_down_to_the_pixel_budget_keeping_its_proportions() {
        let (_, bytes) = screenshot(&picture(700, 6_000)).unwrap();
        let decoded = ::image::load_from_memory(&bytes).unwrap();
        assert!(u64::from(decoded.width()) * u64::from(decoded.height()) <= PIXEL_BUDGET);
        let ratio = f64::from(decoded.height()) / f64::from(decoded.width());
        assert!((ratio - 6000.0 / 700.0).abs() < 0.2, "{ratio}");
        assert!(bytes.len() <= limits::WEB_AGENT_IMAGE_BYTES);
    }

    #[test]
    fn two_pictures_agree_when_only_a_little_of_the_page_moves_by_itself() {
        let still = picture(200, 100);
        assert!(still.agrees_with(&still.clone()));
        let mut little = still.clone();
        // A band of a row, like a clock or a loader.
        for byte in &mut little.bytes[..still.stride] {
            *byte = byte.wrapping_add(90);
        }
        assert!(little.agrees_with(&still));
        let mut much = still.clone();
        for byte in &mut much.bytes[..still.stride * 40] {
            *byte = byte.wrapping_add(90);
        }
        assert!(!much.agrees_with(&still));
        assert!(!picture(200, 99).agrees_with(&still));
    }

    #[test]
    fn a_picture_that_does_not_hold_its_pixels_is_refused() {
        let mut broken = picture(4, 4);
        broken.bytes.truncate(10);
        assert!(screenshot(&broken).is_err());
        let mut narrow = picture(4, 4);
        narrow.stride = 8;
        assert!(screenshot(&narrow).is_err());
    }
}
