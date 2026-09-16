use crate::errors::{self, Result};
use soyeht_pane_state::{
    CursorPosition, HarnessIdentity, MAX_SCREEN_LINE_BYTES, MAX_SCREEN_LINES, MAX_TITLE_BYTES,
    ObservationFrame, ObservationReason,
};
use std::collections::VecDeque;

const BYTE_RATE_WINDOW_MS: i64 = 1_000;
const STATIC_ACTIVITY_HEARTBEAT_MS: i64 = 1_000;

#[derive(Default)]
struct TitleCallbacks {
    title: Option<String>,
}

impl vt100::Callbacks for TitleCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        let title = truncate_utf8(&String::from_utf8_lossy(title), MAX_TITLE_BYTES);
        self.title = (!title.is_empty()).then_some(title);
    }
}

#[derive(Clone, PartialEq, Eq)]
struct ScreenSnapshot {
    title: Option<String>,
    lines: Vec<String>,
    cursor: CursorPosition,
}

/// Bounded, deterministic projection of terminal bytes into experiment rows.
/// It owns no filesystem or process handles; the engine decides when a frame
/// is admitted to the asynchronous recorder.
pub struct TerminalProjection {
    pane_id: String,
    parser: vt100::Parser<TitleCallbacks>,
    cols: u16,
    next_sequence: u64,
    previous: Option<ScreenSnapshot>,
    last_emitted_at_ms: Option<i64>,
    byte_samples: VecDeque<(i64, u64)>,
}

impl TerminalProjection {
    pub fn new(
        pane_id: impl Into<String>,
        rows: u16,
        cols: u16,
        next_sequence: u64,
    ) -> Result<Self> {
        if rows == 0 || cols == 0 {
            return Err(errors::invalid("Terminal dimensions must be positive"));
        }
        let pane_id = pane_id.into();
        let probe = ObservationFrame {
            pane_id: pane_id.clone(),
            harness: HarnessIdentity::Unknown,
            timestamp_ms: 0,
            sequence: next_sequence,
            reason: ObservationReason::VisualChange,
            osc_title: None,
            lines: Vec::new(),
            cursor: None,
            foreground_process: None,
            bytes_per_second: 0,
        };
        probe.validate()?;
        Ok(Self {
            pane_id,
            parser: vt100::Parser::new_with_callbacks(rows, cols, 0, TitleCallbacks::default()),
            cols,
            next_sequence,
            previous: None,
            last_emitted_at_ms: None,
            byte_samples: VecDeque::new(),
        })
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        if rows == 0 || cols == 0 {
            return Err(errors::invalid("Terminal dimensions must be positive"));
        }
        self.parser.screen_mut().set_size(rows, cols);
        self.cols = cols;
        Ok(())
    }

    pub fn process(&mut self, timestamp_ms: i64, bytes: &[u8]) -> Result<Option<ObservationFrame>> {
        if timestamp_ms < 0 {
            return Err(errors::invalid(
                "Observation timestamp must not be negative",
            ));
        }
        let clock_rolled_back = self.record_bytes(timestamp_ms, bytes.len())?;
        self.parser.process(bytes);
        let snapshot = self.snapshot();
        let projection_changed = self.previous.as_ref() != Some(&snapshot);
        let static_activity_due = !bytes.is_empty()
            && (clock_rolled_back
                || self.last_emitted_at_ms.is_none_or(|last| {
                    timestamp_ms.saturating_sub(last) >= STATIC_ACTIVITY_HEARTBEAT_MS
                }));
        if !projection_changed && !static_activity_due {
            return Ok(None);
        }
        let sequence = self.next_sequence;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| errors::internal("Observation sequence exhausted"))?;
        self.previous = Some(snapshot.clone());
        self.last_emitted_at_ms = Some(timestamp_ms);
        let frame = ObservationFrame {
            pane_id: self.pane_id.clone(),
            harness: HarnessIdentity::Unknown,
            timestamp_ms,
            sequence,
            reason: if projection_changed {
                ObservationReason::VisualChange
            } else {
                ObservationReason::StaticActivity
            },
            osc_title: snapshot.title,
            lines: snapshot.lines,
            cursor: Some(snapshot.cursor),
            // Raw PTY output does not identify the foreground process. Unknown
            // is preserved rather than guessed from screen text.
            foreground_process: None,
            bytes_per_second: self.bytes_per_second(),
        };
        frame.validate()?;
        Ok(Some(frame))
    }

    fn snapshot(&self) -> ScreenSnapshot {
        let screen = self.parser.screen();
        let mut lines: Vec<_> = screen
            .rows(0, self.cols)
            .map(|line| truncate_utf8(&line, MAX_SCREEN_LINE_BYTES))
            .collect();
        if lines.len() > MAX_SCREEN_LINES {
            lines.drain(..lines.len() - MAX_SCREEN_LINES);
        }
        let (row, column) = screen.cursor_position();
        ScreenSnapshot {
            title: self.parser.callbacks().title.clone(),
            lines,
            cursor: CursorPosition {
                row: u32::from(row),
                column: u32::from(column),
            },
        }
    }

    fn record_bytes(&mut self, timestamp_ms: i64, byte_count: usize) -> Result<bool> {
        let clock_rolled_back = self
            .byte_samples
            .back()
            .is_some_and(|(previous, _)| timestamp_ms < *previous);
        if clock_rolled_back {
            self.byte_samples.clear();
            self.last_emitted_at_ms = None;
        }
        let byte_count = u64::try_from(byte_count)
            .map_err(|_| errors::internal("Terminal byte count exceeds the supported range"))?;
        self.byte_samples.push_back((timestamp_ms, byte_count));
        let earliest = timestamp_ms.saturating_sub(BYTE_RATE_WINDOW_MS - 1);
        while self
            .byte_samples
            .front()
            .is_some_and(|(sample, _)| *sample < earliest)
        {
            self.byte_samples.pop_front();
        }
        Ok(clock_rolled_back)
    }

    fn bytes_per_second(&self) -> u64 {
        self.byte_samples
            .iter()
            .fold(0_u64, |total, (_, bytes)| total.saturating_add(*bytes))
    }
}

fn truncate_utf8(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.into();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_projection_tracks_title_cursor_and_only_emits_screen_changes() {
        let mut projection = TerminalProjection::new("pane-1", 4, 20, 7).unwrap();
        let first = projection
            .process(1_000, b"\x1b]2;build\x07hello\r\nworld")
            .unwrap()
            .unwrap();
        assert_eq!(first.sequence, 7);
        assert_eq!(first.reason, ObservationReason::VisualChange);
        assert_eq!(first.osc_title.as_deref(), Some("build"));
        assert_eq!(first.lines[0], "hello");
        assert_eq!(first.lines[1], "world");
        assert_eq!(first.cursor, Some(CursorPosition { row: 1, column: 5 }));
        assert_eq!(first.harness, HarnessIdentity::Unknown);
        assert_eq!(first.foreground_process, None);

        assert_eq!(projection.process(1_050, b"\x1b[5Dworld").unwrap(), None);
        let changed = projection.process(1_100, b"!").unwrap().unwrap();
        assert_eq!(changed.sequence, 8);
        assert_eq!(changed.lines[1], "world!");
    }

    #[test]
    fn static_byte_activity_emits_once_per_second_but_absolute_idle_does_not() {
        let mut projection = TerminalProjection::new("pane-1", 4, 20, 0).unwrap();
        projection.process(1_000, b"hello").unwrap().unwrap();

        assert_eq!(projection.process(1_500, b"\x1b[5Dhello").unwrap(), None);
        let heartbeat = projection.process(2_000, b"\x1b[5Dhello").unwrap().unwrap();
        assert_eq!(heartbeat.sequence, 1);
        assert_eq!(heartbeat.reason, ObservationReason::StaticActivity);
        assert_eq!(heartbeat.lines[0], "hello");
        assert!(heartbeat.bytes_per_second > 0);

        assert_eq!(projection.process(3_500, b"").unwrap(), None);
    }

    #[test]
    fn projection_bounds_unicode_title_and_visible_lines() {
        let mut projection = TerminalProjection::new("pane-1", 70, 2_100, 0).unwrap();
        let title = "é".repeat(MAX_TITLE_BYTES);
        let long_line = "é".repeat(MAX_SCREEN_LINE_BYTES);
        let bytes = format!("\x1b]2;{title}\x07{long_line}");
        let frame = projection.process(1, bytes.as_bytes()).unwrap().unwrap();
        assert_eq!(frame.osc_title.as_ref().unwrap().len(), MAX_TITLE_BYTES);
        assert_eq!(frame.lines.len(), MAX_SCREEN_LINES);
        assert!(
            frame
                .lines
                .iter()
                .all(|line| line.len() <= MAX_SCREEN_LINE_BYTES)
        );
        assert!(frame.osc_title.unwrap().is_char_boundary(MAX_TITLE_BYTES));
    }

    #[test]
    fn byte_rate_uses_one_second_window_and_resets_on_clock_rollback() {
        let mut projection = TerminalProjection::new("pane-1", 2, 20, 0).unwrap();
        let first = projection.process(1_000, b"a").unwrap().unwrap();
        assert_eq!(first.bytes_per_second, 1);
        let second = projection.process(1_999, b"b").unwrap().unwrap();
        assert_eq!(second.bytes_per_second, 2);
        let third = projection.process(2_000, b"c").unwrap().unwrap();
        assert_eq!(third.bytes_per_second, 2);
        let rollback = projection.process(500, b"d").unwrap().unwrap();
        assert_eq!(rollback.bytes_per_second, 1);
    }

    #[test]
    fn resize_changes_the_bounded_projection_without_guessing_metadata() {
        let mut projection = TerminalProjection::new("pane-1", 2, 10, 3).unwrap();
        projection.process(1, b"1234567890").unwrap().unwrap();
        projection.resize(3, 5).unwrap();
        let frame = projection.process(2, b"\rX").unwrap().unwrap();
        assert_eq!(frame.lines.len(), 3);
        assert!(frame.lines.iter().any(|line| line.starts_with('X')));
        assert_eq!(frame.foreground_process, None);
    }
}
