//! Local framing. Length excludes the five-byte common header.
use crate::limits;
use std::io::{self, Read, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Json(Vec<u8>),
    Terminal {
        pane_ref: u32,
        generation: u32,
        offset: u64,
        bytes: Vec<u8>,
    },
    Attachment {
        header: [u8; 16],
        bytes: Vec<u8>,
    },
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn validate_length(kind: u8, length: usize) -> io::Result<()> {
    let (minimum, maximum) = match kind {
        1 => (1, limits::JSON_BYTES),
        2 => (
            limits::TERMINAL_HEADER_BYTES,
            limits::TERMINAL_HEADER_BYTES + limits::TERMINAL_PAYLOAD_BYTES,
        ),
        3 => (
            limits::ATTACHMENT_HEADER_BYTES,
            limits::ATTACHMENT_HEADER_BYTES + limits::ATTACHMENT_PAYLOAD_BYTES,
        ),
        _ => return Err(invalid("unknown frame type")),
    };
    if !(minimum..=maximum).contains(&length) {
        return Err(invalid("invalid frame length"));
    }
    Ok(())
}

impl Frame {
    pub fn write_to(&self, mut writer: impl Write) -> io::Result<()> {
        let (kind, header, bytes) = match self {
            Self::Json(bytes) => (1, Vec::new(), bytes),
            Self::Terminal {
                pane_ref,
                generation,
                offset,
                bytes,
            } => {
                let mut header = Vec::with_capacity(limits::TERMINAL_HEADER_BYTES);
                header.extend_from_slice(&pane_ref.to_be_bytes());
                header.extend_from_slice(&generation.to_be_bytes());
                header.extend_from_slice(&offset.to_be_bytes());
                (2, header, bytes)
            }
            Self::Attachment { header, bytes } => (3, header.to_vec(), bytes),
        };
        let length = header
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("length overflow"))?;
        validate_length(kind, length)?;
        writer.write_all(&[kind])?;
        writer.write_all(&(length as u32).to_be_bytes())?;
        writer.write_all(&header)?;
        writer.write_all(bytes)
    }

    /// A clean EOF is None; a truncated header or body is an error.
    /// Validate the announced length before allocating any payload memory.
    pub fn read_from(mut reader: impl Read) -> io::Result<Option<Self>> {
        let mut kind = [0];
        loop {
            match reader.read(&mut kind) {
                Ok(0) => return Ok(None),
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        let mut length = [0; 4];
        reader.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        validate_length(kind[0], length)?;
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes)?;
        let frame = match kind[0] {
            1 => Self::Json(bytes),
            2 => {
                let header: [u8; limits::TERMINAL_HEADER_BYTES] = bytes
                    .get(..limits::TERMINAL_HEADER_BYTES)
                    .ok_or_else(|| invalid("terminal header is missing"))?
                    .try_into()
                    .map_err(|_| invalid("terminal header has the wrong size"))?;
                let pane_ref = u32::from_be_bytes(
                    header[..4]
                        .try_into()
                        .map_err(|_| invalid("terminal pane reference is malformed"))?,
                );
                let generation = u32::from_be_bytes(
                    header[4..8]
                        .try_into()
                        .map_err(|_| invalid("terminal generation is malformed"))?,
                );
                let offset = u64::from_be_bytes(
                    header[8..]
                        .try_into()
                        .map_err(|_| invalid("terminal offset is malformed"))?,
                );
                Self::Terminal {
                    pane_ref,
                    generation,
                    offset,
                    bytes: bytes[limits::TERMINAL_HEADER_BYTES..].to_vec(),
                }
            }
            3 => {
                let header = bytes
                    .get(..limits::ATTACHMENT_HEADER_BYTES)
                    .ok_or_else(|| invalid("attachment header is missing"))?
                    .try_into()
                    .map_err(|_| invalid("attachment header has the wrong size"))?;
                Self::Attachment {
                    header,
                    bytes: bytes[limits::ATTACHMENT_HEADER_BYTES..].to_vec(),
                }
            }
            _ => return Err(invalid("unknown frame type")),
        };
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rejects_length_before_attempting_payload_read() {
        for header in [
            [2, 255, 255, 255, 255],
            [2, 0, 0, 0, 15],
            [0, 0, 0, 0, 1],
            [1, 0, 0, 0, 0],
        ] {
            assert_eq!(
                Frame::read_from(&header[..]).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn empty_stream_is_not_a_truncated_frame() {
        assert_eq!(Frame::read_from(&[][..]).unwrap(), None);
        assert_eq!(
            Frame::read_from(&[1, 0][..]).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn reads_coalesced_frames_and_preserves_high_offsets() {
        let expected = Frame::Terminal {
            pane_ref: 9,
            generation: 4,
            offset: u64::MAX,
            bytes: vec![0, 255, 27],
        };
        let mut bytes = Vec::new();
        expected.write_to(&mut bytes).unwrap();
        Frame::Json(b"{}".to_vec()).write_to(&mut bytes).unwrap();
        let mut reader = Cursor::new(bytes);
        assert_eq!(Frame::read_from(&mut reader).unwrap(), Some(expected));
        assert_eq!(
            Frame::read_from(&mut reader).unwrap(),
            Some(Frame::Json(b"{}".to_vec()))
        );
        assert_eq!(Frame::read_from(&mut reader).unwrap(), None);
    }

    #[test]
    fn exact_terminal_limit_is_accepted_but_next_byte_is_not() {
        let mut frame = Frame::Terminal {
            pane_ref: 1,
            generation: 1,
            offset: 0,
            bytes: vec![0; limits::TERMINAL_PAYLOAD_BYTES],
        };
        let mut encoded = Vec::new();
        frame.write_to(&mut encoded).unwrap();
        assert_eq!(Frame::read_from(&encoded[..]).unwrap(), Some(frame.clone()));
        if let Frame::Terminal { bytes, .. } = &mut frame {
            bytes.push(0);
        }
        assert!(frame.write_to(Vec::new()).is_err());
    }
}
