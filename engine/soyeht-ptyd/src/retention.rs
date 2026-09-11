use crate::wire::{Gap, Output};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::collections::VecDeque;

/// All positions are exclusive byte offsets in one process generation.
pub struct Retention {
    bytes: VecDeque<u8>,
    first: u64,
    next: u64,
    ack: u64,
    limit: usize,
}

impl Retention {
    pub fn new(limit: usize) -> Self {
        assert!(limit > 0);
        Self {
            bytes: VecDeque::new(),
            first: 0,
            next: 0,
            ack: 0,
            limit,
        }
    }

    pub fn append(&mut self, bytes: &[u8]) {
        self.next = self
            .next
            .checked_add(bytes.len() as u64)
            .expect("process output offset overflow");
        for byte in bytes {
            if self.bytes.len() == self.limit {
                self.bytes.pop_front();
                self.first += 1;
            }
            self.bytes.push_back(*byte);
        }
    }

    pub fn acknowledge(&mut self, seq: u64) -> Result<(), &'static str> {
        if seq > self.next {
            return Err("ack_ahead");
        }
        if seq <= self.ack {
            return Ok(());
        }
        self.ack = seq;
        let drop = seq.saturating_sub(self.first).min(self.bytes.len() as u64) as usize;
        self.bytes.drain(..drop);
        self.first += drop as u64;
        Ok(())
    }

    pub fn read(
        &self,
        requested: u64,
        maximum: usize,
        exited: bool,
    ) -> Result<Output, &'static str> {
        if requested > self.next {
            return Err("cursor_ahead");
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
    fn overflow_reports_the_exact_lost_range_and_keeps_new_output() {
        let mut buffer = Retention::new(8);
        buffer.append(b"0123456789ABC");
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
    fn acknowledgement_is_monotonic_and_cannot_discard_unseen_future() {
        let mut buffer = Retention::new(8);
        buffer.append(b"01234567");
        buffer.acknowledge(4).unwrap();
        buffer.acknowledge(2).unwrap();
        assert!(buffer.acknowledge(9).is_err());
        let output = buffer.read(4, 8, false).unwrap();
        assert!(output.gap.is_none());
        assert_eq!(STANDARD.decode(output.bytes).unwrap(), b"4567");
        buffer.acknowledge(8).unwrap();
        buffer.append(b"89");
        assert_eq!(buffer.read(8, 8, false).unwrap().next_seq, 10);
    }
}
