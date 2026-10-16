//! Images are prepared so that the agent can see them: a type the agents do not take inline, such
//! as TIFF or HEIC, or an image too large for one message, is re-encoded as PNG or JPEG, and a
//! small preview is made to keep with the transcript. The limits and the order in which sizes and
//! qualities are tried are the Mac's (`AgentAttachmentStaging`; the shared cases are in
//! `contracts/vectors/attachments.json`).

use image::{
    DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, codecs::jpeg::JpegEncoder,
    imageops::FilterType, metadata::Orientation,
};
use soyeht_protocol::{
    client_policy::INLINE_IMAGE_TYPES, limits::PROMPT_ATTACHMENT_THUMBNAIL_BYTES,
};
use std::io::Cursor;

/// Providers downscale larger images anyway, and the limit keeps an image inside the one-line
/// message once encoded as base64.
pub const IMAGE_MAXIMUM_PIXELS: u32 = 2_048;
pub const IMAGE_MAXIMUM_BYTES: usize = 700 * 1_024;
pub const IMAGE_FALLBACK_NAME: &str = "Pasted image";
pub(super) const THUMBNAIL_PIXELS: u32 = 160;
/// The longest sides an image is scaled to, the largest that fits first.
pub(super) const SCALE_PIXELS: [u32; 5] = [IMAGE_MAXIMUM_PIXELS, 1_600, 1_280, 1_024, 768];
/// The JPEG qualities tried at each scale, the best first, in percent.
pub(super) const JPEG_QUALITIES: [u8; 4] = [85, 70, 55, 40];
/// The preview's longest side and JPEG quality (percent), tried in turn until one fits.
pub(super) const THUMBNAIL_ATTEMPTS: [(u32, u8); 3] =
    [(THUMBNAIL_PIXELS, 70), (THUMBNAIL_PIXELS, 45), (96, 45)];

/// Whether an image goes to the agent as it is: a type every agent takes inline, small enough in
/// bytes and in pixels.
pub fn keeps_original(mime_type: &str, byte_count: usize, width: u32, height: u32) -> bool {
    INLINE_IMAGE_TYPES.contains(&mime_type)
        && byte_count <= IMAGE_MAXIMUM_BYTES
        && width.max(height) <= IMAGE_MAXIMUM_PIXELS
}

/// The name an image is attached under: its own name without the extension, or "Pasted image",
/// with the extension of the type it is sent as.
pub fn image_file_name(name: Option<&str>, sent_as: &str) -> String {
    let base = name.map_or("", without_extension).trim();
    let extension = match sent_as {
        "image/jpeg" => "jpeg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    };
    format!(
        "{}.{extension}",
        if base.is_empty() {
            IMAGE_FALLBACK_NAME
        } else {
            base
        }
    )
}

/// A name without its last extension: "holiday.photo.tiff" is "holiday.photo", ".hidden" stays.
fn without_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < name.len() => &name[..dot],
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

/// What an image is sent as.
pub(super) struct Prepared {
    pub bytes: Vec<u8>,
    pub mime_type: &'static str,
    pub thumbnail: Option<Vec<u8>>,
}

/// An image ready for the agent with its preview, or `None` when the bytes are not an image.
pub(super) fn prepare(data: &[u8], mime_type: &str) -> Option<Prepared> {
    let image = decode(data)?;
    let (width, height) = (image.width(), image.height());
    let thumbnail = thumbnail(&image);
    if keeps_original(mime_type, data.len(), width, height) {
        let mime_type = INLINE_IMAGE_TYPES
            .iter()
            .find(|inline| **inline == mime_type)?;
        return Some(Prepared {
            bytes: data.to_vec(),
            mime_type,
            thumbnail,
        });
    }
    let (bytes, mime_type) = reencode(&image)?;
    Some(Prepared {
        bytes,
        mime_type,
        thumbnail,
    })
}

/// The picture in `data` as the person sees it (turned as its metadata says), or `None` when the
/// bytes are not one: the caller says the file could not be read.
fn decode(data: &[u8]) -> Option<DynamicImage> {
    let Ok(reader) = ImageReader::new(Cursor::new(data)).with_guessed_format() else {
        return None;
    };
    let Ok(mut decoder) = reader.into_decoder() else {
        return None;
    };
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let Ok(mut image) = DynamicImage::from_decoder(decoder) else {
        return None;
    };
    image.apply_orientation(orientation);
    Some(image)
}

/// PNG for a picture with transparency when it fits, else the best JPEG that fits at the
/// largest size that does; the smallest JPEG made when none does.
fn reencode(image: &DynamicImage) -> Option<(Vec<u8>, &'static str)> {
    let mut smallest: Option<Vec<u8>> = None;
    for pixels in SCALE_PIXELS {
        let scaled = scaled(image, pixels, FilterType::Lanczos3);
        if scaled.color().has_alpha()
            && let Some(png) = encode_png(&scaled)
            && png.len() <= IMAGE_MAXIMUM_BYTES
        {
            return Some((png, "image/png"));
        }
        for quality in JPEG_QUALITIES {
            let jpeg = encode_jpeg(&scaled, quality);
            if let Some(jpeg) = jpeg {
                if jpeg.len() <= IMAGE_MAXIMUM_BYTES {
                    return Some((jpeg, "image/jpeg"));
                }
                if smallest.as_ref().is_none_or(|held| jpeg.len() < held.len()) {
                    smallest = Some(jpeg);
                }
            }
        }
    }
    // Still large: the engine links it instead of sending it inline.
    smallest.map(|jpeg| (jpeg, "image/jpeg"))
}

/// A preview within the protocol's thumbnail budget, or none.
fn thumbnail(image: &DynamicImage) -> Option<Vec<u8>> {
    THUMBNAIL_ATTEMPTS.iter().find_map(|(pixels, quality)| {
        encode_jpeg(&scaled(image, *pixels, FilterType::Triangle), *quality)
            .filter(|jpeg| jpeg.len() <= PROMPT_ATTACHMENT_THUMBNAIL_BYTES)
    })
}

/// The image scaled so that its longest side is at most `pixels`; a smaller one is not enlarged.
fn scaled(image: &DynamicImage, pixels: u32, filter: FilterType) -> DynamicImage {
    if image.width().max(image.height()) <= pixels {
        image.clone()
    } else {
        image.resize(pixels, pixels, filter)
    }
}

fn encode_png(image: &DynamicImage) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    match image.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png) {
        Ok(()) => Some(bytes),
        Err(error) => {
            log_encode_failure("png", &error);
            None
        }
    }
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    let written = JpegEncoder::new_with_quality(&mut bytes, quality).write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    );
    match written {
        Ok(()) => Some(bytes),
        Err(error) => {
            log_encode_failure("jpeg", &error);
            None
        }
    }
}

/// An image that cannot be encoded leaves the person a chip without a picture: say why in the
/// log. Only the class of the error: its text can name a file.
fn log_encode_failure(format: &str, error: &image::ImageError) {
    let kind = match error {
        image::ImageError::Decoding(_) => "decoding",
        image::ImageError::Encoding(_) => "encoding",
        image::ImageError::Parameter(_) => "parameter",
        image::ImageError::Limits(_) => "limits",
        image::ImageError::Unsupported(_) => "unsupported",
        image::ImageError::IoError(_) => "io",
    };
    soyeht_platform::log::event(
        "desktop.image_encode_failed",
        &[("format", format), ("kind", kind)],
    );
}
