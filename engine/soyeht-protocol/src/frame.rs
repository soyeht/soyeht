//! Local framing. Length excludes the five-byte common header.
use crate::{Error, Result, limits};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{ErrorKind, Read, Write};

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

pub fn validate_length(kind: u8, length: usize) -> Result<()> {
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
        _ => return Err(Error::UnknownFrameType(kind)),
    };
    if !(minimum..=maximum).contains(&length) {
        return Err(Error::InvalidFrameLength { kind, length });
    }
    Ok(())
}

pub fn write_json(writer: impl Write, value: &impl Serialize) -> Result<()> {
    Frame::Json(serde_json::to_vec(value)?).write_to(writer)
}

pub fn read_json<T: DeserializeOwned>(reader: impl Read) -> Result<Option<T>> {
    match Frame::read_from(reader)? {
        Some(Frame::Json(bytes)) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Some(_) => Err(Error::JsonFrameRequired),
        None => Ok(None),
    }
}

impl Frame {
    pub fn write_to(&self, mut writer: impl Write) -> Result<()> {
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
            .ok_or(Error::FrameLengthOverflow)?;
        validate_length(kind, length)?;
        writer.write_all(&[kind])?;
        writer.write_all(&(length as u32).to_be_bytes())?;
        writer.write_all(&header)?;
        writer.write_all(bytes)?;
        Ok(())
    }

    /// A clean EOF is None; a truncated header or body is an error.
    /// Validate the announced length before allocating any payload memory.
    pub fn read_from(mut reader: impl Read) -> Result<Option<Self>> {
        let mut kind = [0];
        loop {
            match reader.read(&mut kind) {
                Ok(0) => return Ok(None),
                Ok(_) => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
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
                    .ok_or(Error::MissingTerminalHeader)?
                    .try_into()
                    .map_err(|_| Error::MalformedTerminalHeader)?;
                let pane_ref = u32::from_be_bytes(
                    header[..4]
                        .try_into()
                        .map_err(|_| Error::MalformedTerminalHeader)?,
                );
                let generation = u32::from_be_bytes(
                    header[4..8]
                        .try_into()
                        .map_err(|_| Error::MalformedTerminalHeader)?,
                );
                let offset = u64::from_be_bytes(
                    header[8..]
                        .try_into()
                        .map_err(|_| Error::MalformedTerminalHeader)?,
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
                    .ok_or(Error::MissingAttachmentHeader)?
                    .try_into()
                    .map_err(|_| Error::MalformedAttachmentHeader)?;
                Self::Attachment {
                    header,
                    bytes: bytes[limits::ATTACHMENT_HEADER_BYTES..].to_vec(),
                }
            }
            _ => return Err(Error::UnknownFrameType(kind[0])),
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
            assert!(matches!(
                Frame::read_from(&header[..]).unwrap_err(),
                Error::InvalidFrameLength { .. } | Error::UnknownFrameType(_)
            ));
        }
    }

    #[test]
    fn empty_stream_is_not_a_truncated_frame() {
        assert_eq!(Frame::read_from(&[][..]).unwrap(), None);
        assert!(matches!(
            Frame::read_from(&[1, 0][..]).unwrap_err(),
            Error::Io(error) if error.kind() == ErrorKind::UnexpectedEof
        ));
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
    fn json_frame_codec_round_trips_values_and_rejects_binary_frames() {
        let value = serde_json::json!({"id": 7, "method": "status"});
        let mut bytes = Vec::new();
        write_json(&mut bytes, &value).unwrap();
        assert_eq!(
            read_json::<serde_json::Value>(&bytes[..]).unwrap(),
            Some(value)
        );

        let mut bytes = Vec::new();
        Frame::Terminal {
            pane_ref: 1,
            generation: 2,
            offset: 3,
            bytes: b"output".to_vec(),
        }
        .write_to(&mut bytes)
        .unwrap();
        assert!(matches!(
            read_json::<serde_json::Value>(&bytes[..]),
            Err(Error::JsonFrameRequired)
        ));
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
