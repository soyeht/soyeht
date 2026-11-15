//! The size of a PNG without decoding it: the package validator compares it with what the
//! package declares.

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// `(width, height)` read from the header chunk of a PNG, or `None` when the bytes are not one.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let header = bytes.get(..24)?;
    if header[..8] != SIGNATURE || &header[12..16] != b"IHDR" {
        return None;
    }
    let word = |at: usize| {
        u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    Some((word(16), word(20)))
}
