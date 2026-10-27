use crate::Frame;

impl Frame {
    /// A picture read out of the engine's surface: premultiplied blue, green, red and alpha. The
    /// page is shown on a white sheet, as a browser shows it, so what is not painted is white
    /// and the picture has no transparency left.
    pub(crate) fn from_surface(
        width: usize,
        height: usize,
        stride: usize,
        scale: u32,
        data: &[u8],
    ) -> Option<Self> {
        let row = width.checked_mul(4)?;
        let length = stride.checked_mul(height)?;
        if width == 0 || height == 0 || stride < row || data.len() < length {
            return None;
        }
        let mut bytes = data[..length].to_vec();
        for line in bytes.chunks_exact_mut(stride) {
            for pixel in line[..row].as_chunks_mut::<4>().0 {
                let cover = 255 - pixel[3];
                pixel[0] = pixel[0].saturating_add(cover);
                pixel[1] = pixel[1].saturating_add(cover);
                pixel[2] = pixel[2].saturating_add(cover);
                pixel[3] = 255;
            }
        }
        let (Ok(wide), Ok(high)) = (u32::try_from(width), u32::try_from(height)) else {
            return None;
        };
        Some(Self {
            width: wide,
            height: high,
            stride,
            bytes,
            scale: scale.max(1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_painted_is_white_and_painted_pixels_are_kept() {
        // Two pixels: fully transparent, and an opaque red (blue, green, red, alpha).
        let data = [0, 0, 0, 0, 0, 0, 200, 255];
        let frame = Frame::from_surface(2, 1, 8, 1, &data).unwrap();
        assert_eq!(frame.bytes, [255, 255, 255, 255, 0, 0, 200, 255]);
        assert_eq!((frame.width, frame.height, frame.scale), (2, 1, 1));
    }

    #[test]
    fn a_surface_that_does_not_hold_its_pixels_makes_no_picture() {
        assert!(Frame::from_surface(2, 2, 8, 1, &[0; 8]).is_none());
        assert!(Frame::from_surface(2, 1, 4, 1, &[0; 8]).is_none());
        assert!(Frame::from_surface(0, 1, 4, 1, &[]).is_none());
    }

    #[test]
    fn the_scale_is_at_least_one() {
        assert_eq!(Frame::from_surface(1, 1, 4, 0, &[0; 4]).unwrap().scale, 1);
    }
}
