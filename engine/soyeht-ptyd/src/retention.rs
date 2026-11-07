use crate::{
    Error, Result,
    wire::{Gap, Output},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::collections::VecDeque;

/// What a buffer keeps allocated for the next output once the engine has taken the rest in is
/// twice this (`SHRINK_ABOVE`). A burst that filled the buffer (up to the limit) would otherwise
/// leave that much memory held for as long as the process lives.
const KEPT_CAPACITY: usize = 64 * 1024;
/// The buffer is shrunk, to this, only when it has grown past it, so a burst a little over what
/// is kept does not grow it and shrink it again on every acknowledgement.
const SHRINK_ABOVE: usize = 2 * KEPT_CAPACITY;

/// All positions are exclusive byte offsets in one process generation.
pub struct Retention {
    bytes: VecDeque<u8>,
    first: u64,
    next: u64,
    ack: u64,
    limit: usize,
}

impl Retention {
    pub fn new(limit: usize) -> Result<Self> {
        if limit == 0 {
            return Err(Error::ZeroRetentionLimit);
        }
        Ok(Self {
            bytes: VecDeque::new(),
            first: 0,
            next: 0,
            ack: 0,
            limit,
        })
    }

    pub fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.next = self
            .next
            .checked_add(bytes.len() as u64)
            .ok_or(Error::OutputOffsetExhausted)?;
        for byte in bytes {
            if self.bytes.len() == self.limit {
                self.bytes.pop_front();
                self.first += 1;
            }
            self.bytes.push_back(*byte);
        }
        Ok(())
    }

    pub fn acknowledge(&mut self, seq: u64) -> Result<()> {
        if seq > self.next {
            return Err(Error::AckAhead);
        }
        if seq <= self.ack {
            return Ok(());
        }
        self.ack = seq;
        let drop = seq.saturating_sub(self.first).min(self.bytes.len() as u64) as usize;
        self.bytes.drain(..drop);
        self.first += drop as u64;
        if self.bytes.capacity() > SHRINK_ABOVE && self.bytes.len() * 4 <= self.bytes.capacity() {
            self.bytes.shrink_to(self.bytes.len().max(SHRINK_ABOVE));
        }
        Ok(())
    }

    /// The end of what has been appended.
    pub fn end_seq(&self) -> u64 {
        self.next
    }

    pub fn read(&self, requested: u64, maximum: usize, exited: bool) -> Result<Output> {
        if requested > self.next {
            return Err(Error::CursorAhead);
        }
        let from_seq = requested.max(self.first);
        let start = (from_seq - self.first) as usize;
        let end = start.saturating_add(maximum).min(self.bytes.len());
        let bytes: Vec<u8> = self.bytes.range(start..end).copied().collect();
        Ok(Output {
            from_seq,
            next_seq: from_seq + bytes.len() as u64,
            end_seq: self.next,
            bytes: STANDARD.encode(&bytes),
            exited,
            gap: (requested < self.first).then_some(Gap {
                from_seq: requested,
                to_seq: self.first,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_capacity_and_offset_exhaustion_return_errors() {
        assert!(matches!(Retention::new(0), Err(Error::ZeroRetentionLimit)));

        let mut buffer = Retention::new(8).unwrap();
        buffer.next = u64::MAX;
        assert!(matches!(
            buffer.append(b"x"),
            Err(Error::OutputOffsetExhausted)
        ));
        assert!(buffer.bytes.is_empty());
    }

    #[test]
    fn overflow_reports_the_exact_lost_range_and_keeps_new_output() {
        let mut buffer = Retention::new(8).unwrap();
        buffer.append(b"0123456789ABC").unwrap();
        let output = buffer.read(0, 4, false).unwrap();
        assert_eq!(
            output.gap,
            Some(Gap {
                from_seq: 0,
                to_seq: 5
            })
        );
        assert_eq!(STANDARD.decode(output.bytes).unwrap(), b"5678");
        assert_eq!(output.next_seq, 9);
        let next = buffer.read(9, 4, false).unwrap();
        assert!(next.gap.is_none());
        assert_eq!(STANDARD.decode(next.bytes).unwrap(), b"9ABC");
    }

    #[test]
    fn acknowledged_output_gives_its_memory_back() {
        let mut buffer = Retention::new(1 << 20).unwrap();
        buffer.append(&vec![b'x'; 1 << 20]).unwrap();
        assert!(buffer.bytes.capacity() >= 1 << 20);

        buffer.acknowledge((1 << 20) - 100).unwrap();

        assert!(buffer.bytes.capacity() <= SHRINK_ABOVE);
        let output = buffer.read((1 << 20) - 100, 1 << 20, false).unwrap();
        assert_eq!(STANDARD.decode(output.bytes).unwrap(), vec![b'x'; 100]);
        // What is still unacknowledged is kept, and so is the room for more.
        buffer.append(&vec![b'y'; 1 << 19]).unwrap();
        assert_eq!(
            buffer.read(1 << 20, 1 << 20, false).unwrap().next_seq,
            (1 << 20) + (1 << 19)
        );
    }

    #[test]
    fn a_burst_a_little_over_what_is_kept_does_not_shrink_the_buffer_again() {
        let mut buffer = Retention::new(1 << 20).unwrap();
        let burst = vec![b'x'; KEPT_CAPACITY + KEPT_CAPACITY / 2];
        let mut kept = None;
        for round in 1..=4 {
            buffer.append(&burst).unwrap();
            // The room the last round left is the room this one used: nothing grew again.
            if let Some(capacity) = kept {
                assert_eq!(buffer.bytes.capacity(), capacity, "round {round}");
            }
            buffer.acknowledge((round * burst.len()) as u64).unwrap();
            kept = Some(buffer.bytes.capacity());
        }
        assert!(buffer.bytes.capacity() <= SHRINK_ABOVE);
    }

    #[test]
    fn acknowledgement_is_monotonic_and_cannot_discard_unseen_future() {
        let mut buffer = Retention::new(8).unwrap();
        buffer.append(b"01234567").unwrap();
        buffer.acknowledge(4).unwrap();
        buffer.acknowledge(2).unwrap();
        assert!(buffer.acknowledge(9).is_err());
        let output = buffer.read(4, 8, false).unwrap();
        assert!(output.gap.is_none());
        assert_eq!(STANDARD.decode(output.bytes).unwrap(), b"4567");
        buffer.acknowledge(8).unwrap();
        buffer.append(b"89").unwrap();
        assert_eq!(buffer.read(8, 8, false).unwrap().next_seq, 10);
    }
}
