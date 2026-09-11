//! Bounded newline-delimited records with durable byte offsets.

use crate::{Error, Result, limits};

#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub from_seq: u64,
    pub to_seq: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Batch {
    pub lines: Vec<Line>,
    /// True only when this batch first crossed the line-size limit.
    pub oversized: bool,
}

/// Incrementally separates one bounded NDJSON stream.
///
/// An oversized record is discarded through its newline. Later records remain
/// readable, and memory use never grows beyond the public ACP message limit.
#[derive(Debug)]
pub struct Decoder {
    bytes: Vec<u8>,
    start: u64,
    next: u64,
    discarding: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Decoder {
    pub fn new(offset: u64) -> Self {
        Self {
            bytes: Vec::new(),
            start: offset,
            next: offset,
            discarding: false,
        }
    }

    pub fn next_offset(&self) -> u64 {
        self.next
    }

    pub fn buffered_len(&self) -> usize {
        self.bytes.len()
    }

    pub fn push(&mut self, offset: u64, bytes: &[u8]) -> Result<Batch> {
        if offset != self.next {
            return Err(Error::NdjsonStreamDiscontinuity {
                expected: self.next,
                actual: offset,
            });
        }
        let mut lines = Vec::new();
        let mut oversized = false;
        for &byte in bytes {
            self.next = self
                .next
                .checked_add(1)
                .ok_or(Error::NdjsonOffsetExhausted)?;
            if byte == b'\n' {
                if !self.discarding {
                    lines.push(Line {
                        from_seq: self.start,
                        to_seq: self.next,
                        bytes: std::mem::take(&mut self.bytes),
                    });
                }
                self.discarding = false;
                self.start = self.next;
            } else if !self.discarding {
                if self.bytes.len() == limits::ACP_MESSAGE_BYTES {
                    self.bytes.clear();
                    self.discarding = true;
                    oversized = true;
                } else {
                    self.bytes.push(byte);
                }
            }
        }
        Ok(Batch { lines, oversized })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_line_is_bounded_discarded_and_followed_by_an_exact_record() {
        let mut decoder = Decoder::new(41);
        let exact = vec![b'x'; limits::ACP_MESSAGE_BYTES];
        let batch = decoder.push(41, &exact).unwrap();
        assert!(batch.lines.is_empty());
        assert!(!batch.oversized);
        assert_eq!(decoder.buffered_len(), limits::ACP_MESSAGE_BYTES);

        let batch = decoder
            .push(decoder.next_offset(), b"y\n{\"ok\":true}\n")
            .unwrap();
        assert!(batch.oversized);
        assert_eq!(decoder.buffered_len(), 0);
        assert_eq!(batch.lines.len(), 1);
        assert_eq!(batch.lines[0].bytes, br#"{"ok":true}"#);
        assert_eq!(
            batch.lines[0].from_seq,
            41 + limits::ACP_MESSAGE_BYTES as u64 + 2
        );
        assert_eq!(batch.lines[0].to_seq, decoder.next_offset());

        let expected = decoder.next_offset();
        assert!(matches!(
            decoder.push(expected - 1, b"{}\n"),
            Err(Error::NdjsonStreamDiscontinuity {
                expected: value,
                actual
            }) if value == expected && actual == expected - 1
        ));
    }
}
