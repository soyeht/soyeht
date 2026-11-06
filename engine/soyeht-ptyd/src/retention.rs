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
        // What the ring holds after the chunk is the last `limit` bytes of everything it held
        // plus the chunk: the oldest bytes go first, and a chunk longer than the limit loses its
        // own head.
        let over = (self.bytes.len() + bytes.len()).saturating_sub(self.limit);
        let from_ring = over.min(self.bytes.len());
        let from_chunk = over - from_ring;
        self.bytes.drain(..from_ring);
        self.bytes.extend(&bytes[from_chunk..]);
        self.first += over as u64;
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

    /// `start..end` of the ring, copied out in at most two slices.
    fn copy_range(&self, start: usize, end: usize) -> Vec<u8> {
        let end = end.min(self.bytes.len());
        let start = start.min(end);
        let (head, tail) = self.bytes.as_slices();
        let mut copied = Vec::with_capacity(end - start);
        if start < head.len() {
            copied.extend_from_slice(&head[start..end.min(head.len())]);
        }
        if end > head.len() {
            copied.extend_from_slice(&tail[start.saturating_sub(head.len())..end - head.len()]);
        }
        copied
    }

    pub fn read(&self, requested: u64, maximum: usize, exited: bool) -> Result<Output> {
        if requested > self.next {
            return Err(Error::CursorAhead);
        }
        let from_seq = requested.max(self.first);
        let start = (from_seq - self.first) as usize;
        let end = start.saturating_add(maximum).min(self.bytes.len());
        let bytes = self.copy_range(start, end);
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

    /// The ring as it was written one byte at a time: the meaning `Retention` keeps.
    struct OneByteAtATime {
        bytes: VecDeque<u8>,
        first: u64,
        next: u64,
        ack: u64,
        limit: usize,
    }

    impl OneByteAtATime {
        fn append(&mut self, chunk: &[u8]) {
            self.next += chunk.len() as u64;
            for byte in chunk {
                if self.bytes.len() == self.limit {
                    self.bytes.pop_front();
                    self.first += 1;
                }
                self.bytes.push_back(*byte);
            }
        }

        fn acknowledge(&mut self, seq: u64) -> bool {
            if seq > self.next {
                return false;
            }
            if seq > self.ack {
                self.ack = seq;
                while self.first < seq && self.bytes.pop_front().is_some() {
                    self.first += 1;
                }
            }
            true
        }

        fn read(&self, requested: u64, maximum: usize) -> Option<(u64, Vec<u8>, Option<Gap>)> {
            if requested > self.next {
                return None;
            }
            let from_seq = requested.max(self.first);
            let skip = (from_seq - self.first) as usize;
            let bytes = self.bytes.iter().skip(skip).take(maximum).copied().collect();
            let gap = (requested < self.first).then_some(Gap {
                from_seq: requested,
                to_seq: self.first,
            });
            Some((from_seq, bytes, gap))
        }
    }

    /// A small deterministic generator, so a failing sequence repeats.
    struct Sequence(u64);

    impl Sequence {
        fn below(&mut self, bound: u64) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) % bound
        }
    }

    #[test]
    fn random_appends_reads_and_acknowledgements_match_the_one_byte_model() {
        for seed in 0..300 {
            let mut random = Sequence(seed);
            let limit = 1 + random.below(40) as usize;
            let mut buffer = Retention::new(limit).unwrap();
            let mut model = OneByteAtATime {
                bytes: VecDeque::new(),
                first: 0,
                next: 0,
                ack: 0,
                limit,
            };
            let mut value = 0u8;
            for step in 0..400 {
                let context = format!("seed {seed} step {step} limit {limit}");
                match random.below(5) {
                    // Chunks of every size around the limit: empty, short, exact, past it.
                    0 | 1 => {
                        let length = match random.below(6) {
                            0 => 0,
                            1 => limit.saturating_sub(1),
                            2 => limit,
                            3 => limit + 1 + random.below(2 * limit as u64) as usize,
                            _ => random.below(limit as u64 + 3) as usize,
                        };
                        let chunk: Vec<u8> = (0..length)
                            .map(|_| {
                                value = value.wrapping_add(1);
                                value
                            })
                            .collect();
                        buffer.append(&chunk).unwrap();
                        model.append(&chunk);
                    }
                    2 => {
                        // Behind, at and past what was written, so the refused ones are tried.
                        let seq = random.below(model.next + 3);
                        assert_eq!(
                            buffer.acknowledge(seq).is_ok(),
                            model.acknowledge(seq),
                            "{context}"
                        );
                    }
                    _ => {
                        let requested = random.below(model.next + 3);
                        let maximum = random.below(limit as u64 + 5) as usize;
                        let read = buffer.read(requested, maximum, false);
                        let Some((from_seq, bytes, gap)) = model.read(requested, maximum) else {
                            assert!(matches!(read, Err(Error::CursorAhead)), "{context}");
                            continue;
                        };
                        let output = read.unwrap();
                        assert_eq!(output.from_seq, from_seq, "{context}");
                        assert_eq!(output.next_seq, from_seq + bytes.len() as u64, "{context}");
                        assert_eq!(output.end_seq, model.next, "{context}");
                        assert_eq!(STANDARD.decode(output.bytes).unwrap(), bytes, "{context}");
                        assert_eq!(output.gap, gap, "{context}");
                    }
                }
                assert_eq!(buffer.first, model.first, "{context}");
                assert_eq!(buffer.next, model.next, "{context}");
                assert!(buffer.bytes.iter().eq(model.bytes.iter()), "{context}");
            }
        }
    }
}
