//! Terminal stream positions. Binary frames name a subscription by a connection-local
//! reference, replay events name it by stream id; both indexes lead back to the pane that owns
//! the position, so interleaved output can never reach another pane.

use crate::{Error, Result};
use soyeht_protocol::{StreamGapEvent, StreamInfo, StreamReplayDoneEvent, StreamReplayStartEvent};
use std::collections::HashMap;

/// What a stream produced for the pane that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamOutput {
    /// A replay that starts from scratch: the screen must be cleared first.
    Reset {
        pane_id: String,
    },
    Bytes {
        pane_id: String,
        bytes: Vec<u8>,
    },
    /// Output the engine no longer retains was skipped.
    Gap {
        pane_id: String,
    },
}

impl StreamOutput {
    /// The pane the output belongs to.
    pub fn pane_id(&self) -> &str {
        match self {
            Self::Reset { pane_id } | Self::Bytes { pane_id, .. } | Self::Gap { pane_id } => {
                pane_id
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Position {
    stream_id: String,
    reference: u32,
    generation: u32,
    next: Option<u64>,
}

impl Position {
    fn start(&mut self, event: &StreamReplayStartEvent) -> Result<bool> {
        let from = decimal(&event.from_offset)?;
        let to = decimal(&event.to_offset)?;
        if event.stream_id != self.stream_id
            || event.generation != self.generation
            || from > to
            || !(event.reset || self.next == Some(from))
        {
            return Err(Error::Invalid("replay start does not continue the stream"));
        }
        self.next = Some(from);
        Ok(event.reset)
    }

    fn accept(&mut self, generation: u32, offset: u64, length: usize) -> Result<()> {
        let out_of_order = || Error::Invalid("terminal frame is out of order");
        if generation != self.generation || self.next != Some(offset) {
            return Err(out_of_order());
        }
        let end = u64::try_from(length)
            .map_err(|_| out_of_order())
            .and_then(|length| offset.checked_add(length).ok_or_else(out_of_order))?;
        self.next = Some(end);
        Ok(())
    }

    fn gap(&mut self, event: &StreamGapEvent) -> Result<bool> {
        let from = decimal(&event.from_offset)?;
        let to = decimal(&event.to_offset)?;
        if event.stream_id != self.stream_id || self.next != Some(from) || to < from {
            return Err(Error::Invalid("gap does not continue the stream"));
        }
        self.next = Some(to);
        Ok(to > from)
    }
}

fn decimal(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| Error::Invalid("offset is not a decimal"))
}

/// Keeps every terminal stream of one connection independent.
#[derive(Debug, Default)]
pub struct TerminalStreams {
    positions: HashMap<String, Position>,
    pane_by_reference: HashMap<u32, String>,
    pane_by_stream: HashMap<String, String>,
}

impl TerminalStreams {
    /// Records the answer to `pane.stream.subscribe`.
    pub fn install(&mut self, info: &StreamInfo) -> Result<()> {
        let pane_id = &info.pane_id;
        let reference_free = self
            .pane_by_reference
            .get(&info.binary_pane_ref)
            .is_none_or(|owner| owner == pane_id);
        let stream_free = self
            .pane_by_stream
            .get(&info.stream_id)
            .is_none_or(|owner| owner == pane_id);
        if !(reference_free && stream_free) {
            return Err(Error::Invalid("stream identity is already in use"));
        }
        self.remove(pane_id);
        self.positions.insert(
            pane_id.clone(),
            Position {
                stream_id: info.stream_id.clone(),
                reference: info.binary_pane_ref,
                generation: info.generation,
                next: None,
            },
        );
        self.pane_by_reference
            .insert(info.binary_pane_ref, pane_id.clone());
        self.pane_by_stream
            .insert(info.stream_id.clone(), pane_id.clone());
        Ok(())
    }

    /// The pane that owns a stream index entry, with its position, if both still exist.
    fn tracked(&mut self, owner: Option<String>) -> Option<(String, &mut Position)> {
        let pane_id = owner?;
        let position = self.positions.get_mut(&pane_id)?;
        Some((pane_id, position))
    }

    /// Runs a replay event against the position of the stream it names. `step` says whether
    /// the event changed what the pane shows, and then `output` tells the pane so.
    fn replay(
        &mut self,
        stream_id: &str,
        step: impl FnOnce(&mut Position) -> Result<bool>,
        output: fn(String) -> StreamOutput,
    ) -> Result<Option<StreamOutput>> {
        let owner = self.pane_by_stream.get(stream_id).cloned();
        let Some((pane_id, position)) = self.tracked(owner) else {
            return Ok(None);
        };
        Ok(step(position)?.then(|| output(pane_id)))
    }

    /// Binary output. Frames of a reference nobody subscribed to are ignored.
    pub fn accept(
        &mut self,
        reference: u32,
        generation: u32,
        offset: u64,
        bytes: Vec<u8>,
    ) -> Result<Option<StreamOutput>> {
        let owner = self.pane_by_reference.get(&reference).cloned();
        let Some((pane_id, position)) = self.tracked(owner) else {
            return Ok(None);
        };
        position.accept(generation, offset, bytes.len())?;
        Ok(Some(StreamOutput::Bytes { pane_id, bytes }))
    }

    pub fn start(&mut self, event: &StreamReplayStartEvent) -> Result<Option<StreamOutput>> {
        self.replay(
            &event.stream_id,
            |position| position.start(event),
            |pane_id| StreamOutput::Reset { pane_id },
        )
    }

    pub fn gap(&mut self, event: &StreamGapEvent) -> Result<Option<StreamOutput>> {
        self.replay(
            &event.stream_id,
            |position| position.gap(event),
            |pane_id| StreamOutput::Gap { pane_id },
        )
    }

    pub fn finish(&self, event: &StreamReplayDoneEvent) -> Result<()> {
        let Some(position) = self
            .pane_by_stream
            .get(&event.stream_id)
            .and_then(|pane_id| self.positions.get(pane_id))
        else {
            return Ok(());
        };
        if position.next.map(|next| next.to_string()).as_deref() == Some(&event.next_offset) {
            Ok(())
        } else {
            Err(Error::Invalid("replay ended at an unexpected offset"))
        }
    }

    /// Forgets a pane's stream, as its position must not outlive the pane.
    pub fn remove(&mut self, pane_id: &str) {
        let Some(position) = self.positions.remove(pane_id) else {
            return;
        };
        if self
            .pane_by_reference
            .get(&position.reference)
            .map(String::as_str)
            == Some(pane_id)
        {
            self.pane_by_reference.remove(&position.reference);
        }
        if self
            .pane_by_stream
            .get(&position.stream_id)
            .map(String::as_str)
            == Some(pane_id)
        {
            self.pane_by_stream.remove(&position.stream_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pane: &str, stream: &str, reference: u32) -> StreamInfo {
        StreamInfo {
            stream_id: stream.into(),
            pane_id: pane.into(),
            generation: 3,
            first_retained_offset: "0".into(),
            next_offset: "0".into(),
            binary_pane_ref: reference,
        }
    }

    fn start(stream: &str, from: u64, to: u64, reset: bool) -> StreamReplayStartEvent {
        StreamReplayStartEvent {
            stream_id: stream.into(),
            generation: 3,
            from_offset: from.to_string(),
            to_offset: to.to_string(),
            reset,
        }
    }

    #[test]
    fn replay_then_live_bytes_arrive_in_order() {
        let mut streams = TerminalStreams::default();
        streams.install(&info("p1", "s1", 7)).unwrap();
        let reset = streams.start(&start("s1", 0, 4, true)).unwrap();
        assert_eq!(
            reset,
            Some(StreamOutput::Reset {
                pane_id: "p1".into()
            })
        );
        let first = streams.accept(7, 3, 0, b"ab".to_vec()).unwrap();
        assert_eq!(
            first,
            Some(StreamOutput::Bytes {
                pane_id: "p1".into(),
                bytes: b"ab".to_vec()
            })
        );
        streams.accept(7, 3, 2, b"cd".to_vec()).unwrap();
        streams
            .finish(&StreamReplayDoneEvent {
                stream_id: "s1".into(),
                next_offset: "4".into(),
            })
            .unwrap();
    }

    #[test]
    fn out_of_order_or_stale_generation_frames_are_refused() {
        let mut streams = TerminalStreams::default();
        streams.install(&info("p1", "s1", 7)).unwrap();
        streams.start(&start("s1", 0, 0, true)).unwrap();
        assert!(streams.accept(7, 3, 5, vec![1]).is_err());
        assert!(streams.accept(7, 2, 0, vec![1]).is_err());
    }

    #[test]
    fn frames_of_unknown_references_are_ignored() {
        let mut streams = TerminalStreams::default();
        assert_eq!(streams.accept(9, 1, 0, vec![1]).unwrap(), None);
    }

    #[test]
    fn interleaved_streams_stay_with_their_panes() {
        let mut streams = TerminalStreams::default();
        streams.install(&info("p1", "s1", 1)).unwrap();
        streams.install(&info("p2", "s2", 2)).unwrap();
        streams.start(&start("s1", 0, 0, true)).unwrap();
        streams.start(&start("s2", 0, 0, true)).unwrap();
        let second = streams.accept(2, 3, 0, b"x".to_vec()).unwrap();
        assert_eq!(
            second,
            Some(StreamOutput::Bytes {
                pane_id: "p2".into(),
                bytes: b"x".to_vec()
            })
        );
        let first = streams.accept(1, 3, 0, b"y".to_vec()).unwrap();
        assert_eq!(
            first,
            Some(StreamOutput::Bytes {
                pane_id: "p1".into(),
                bytes: b"y".to_vec()
            })
        );
    }

    #[test]
    fn a_reference_cannot_be_claimed_by_two_panes() {
        let mut streams = TerminalStreams::default();
        streams.install(&info("p1", "s1", 1)).unwrap();
        assert!(streams.install(&info("p2", "s2", 1)).is_err());
    }

    #[test]
    fn gap_skips_unretained_output_and_reports_it_once() {
        let mut streams = TerminalStreams::default();
        streams.install(&info("p1", "s1", 1)).unwrap();
        streams.start(&start("s1", 0, 0, true)).unwrap();
        let gap = StreamGapEvent {
            stream_id: "s1".into(),
            from_offset: "0".into(),
            to_offset: "10".into(),
        };
        assert_eq!(
            streams.gap(&gap).unwrap(),
            Some(StreamOutput::Gap {
                pane_id: "p1".into()
            })
        );
        streams.accept(1, 3, 10, vec![1]).unwrap();
    }

    #[test]
    fn continuing_replay_must_start_where_the_last_one_ended() {
        let mut streams = TerminalStreams::default();
        streams.install(&info("p1", "s1", 1)).unwrap();
        streams.start(&start("s1", 0, 2, true)).unwrap();
        streams.accept(1, 3, 0, b"ab".to_vec()).unwrap();
        assert!(streams.start(&start("s1", 5, 6, false)).is_err());
        assert_eq!(streams.start(&start("s1", 2, 4, false)).unwrap(), None);
    }

    #[test]
    fn removed_panes_release_their_identity() {
        let mut streams = TerminalStreams::default();
        streams.install(&info("p1", "s1", 1)).unwrap();
        streams.remove("p1");
        // Its reference and stream id are free for another pane, and its frames are ignored.
        assert_eq!(streams.accept(1, 1, 0, b"x".to_vec()).unwrap(), None);
        streams.install(&info("p2", "s2", 1)).unwrap();
    }
}
