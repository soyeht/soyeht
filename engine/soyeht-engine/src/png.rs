//! What the engine checks of a PNG file it is sent, with no decoder: the signature, and the size
//! the header declares. One place, for every attachment that must be a PNG.

/// The media type of a PNG file.
pub(crate) const MIME: &str = "image/png";

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// The signature, then the `IHDR` chunk: its length (13), its name, the width and the height.
const HEADER_BYTES: usize = 24;

/// Whether `bytes` begin with the PNG signature.
pub(crate) fn has_signature(bytes: &[u8]) -> bool {
    bytes.starts_with(SIGNATURE)
}

/// The width and height the header of the PNG file declares, when it has the signature and a
/// well-formed `IHDR` chunk first, as every PNG file does. Neither is zero. The rest of the file
/// is not read.
pub(crate) fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let header = bytes.get(..HEADER_BYTES)?;
    if !has_signature(header) || header[8..12] != [0, 0, 0, 13] || &header[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// The signature and header of a PNG file of the size given, and nothing after it: what the
/// checks above read, for the tests that need a picture.
#[cfg(test)]
pub(crate) fn header_of(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = SIGNATURE.to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 13]);
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: u32, height: u32) -> Vec<u8> {
        header_of(width, height)
    }

    #[test]
    fn the_header_names_the_size() {
        assert_eq!(dimensions(&header(640, 480)), Some((640, 480)));
    }

    #[test]
    fn a_file_that_is_not_a_png_header_has_no_size() {
        assert_eq!(dimensions(SIGNATURE), None);
        assert_eq!(dimensions(&header(0, 480)), None);
        assert_eq!(dimensions(&header(640, 0)), None);
        let mut other_chunk = header(1, 1);
        other_chunk[12..16].copy_from_slice(b"IDAT");
        assert_eq!(dimensions(&other_chunk), None);
        let mut wrong_length = header(1, 1);
        wrong_length[11] = 12;
        assert_eq!(dimensions(&wrong_length), None);
        let mut gif = header(1, 1);
        gif[1] = b'X';
        assert_eq!(dimensions(&gif), None);
        assert!(!has_signature(b""));
    }
}
