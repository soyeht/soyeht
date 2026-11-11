//! What typing or pasting into a terminal pane does to the replica and what it sends.

use crate::{Command, Connection, State};

/// What the bytes typed into a terminal become.
#[derive(Debug, Clone, PartialEq)]
pub enum Typed {
    /// The engine cannot be reached. The person was told, and nothing was sent.
    Refused,
    /// The request that carries the bytes; `redraw` says that the screen changed (a selection
    /// ended, or the view went back to the live bottom) and has to be drawn again.
    Send { command: Command, redraw: bool },
}

impl State {
    /// Bytes typed or pasted into the terminal `pane_id`. What is typed ends a selection and
    /// returns the view to the live bottom, as in every terminal.
    pub fn type_into_terminal(&mut self, pane_id: &str, bytes: Vec<u8>) -> Typed {
        if self.connection != Connection::Connected {
            // Keystrokes cannot reach a dead connection; say so instead of dropping them.
            self.report_disconnected();
            return Typed::Refused;
        }
        let mut redraw = false;
        if let Some(screen) = self.screen_mut(pane_id) {
            let selected = screen.has_selection();
            screen.clear_selection();
            let scrolled = screen.scrolled_back();
            if scrolled {
                screen.scroll_to_bottom();
            }
            redraw = selected || scrolled;
        }
        Typed::Send {
            command: Command::Input {
                pane_id: pane_id.to_owned(),
                bytes,
            },
            redraw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::connected;

    fn input(bytes: &[u8]) -> Command {
        Command::Input {
            pane_id: "t".into(),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn typing_sends_the_bytes_to_the_pane() {
        let mut state = connected(&[("t", "terminal")]);
        assert_eq!(
            state.type_into_terminal("t", b"ls\r".to_vec()),
            Typed::Send {
                command: input(b"ls\r"),
                redraw: false
            }
        );
    }

    #[test]
    fn typing_returns_a_view_that_looks_at_history_to_the_live_bottom() {
        let mut state = connected(&[("t", "terminal")]);
        let output: String = (0..100).map(|line| format!("line {line}\r\n")).collect();
        let screen = state.screen_mut("t").unwrap();
        screen.feed(output.as_bytes());
        assert!(screen.scroll_by(10.0) && screen.scrolled_back());
        assert_eq!(
            state.type_into_terminal("t", b"q".to_vec()),
            Typed::Send {
                command: input(b"q"),
                redraw: true
            }
        );
        assert!(!state.screen("t").unwrap().scrolled_back());
    }

    #[test]
    fn typing_on_a_dead_connection_sends_nothing_and_says_so() {
        let mut state = crate::State::default();
        assert_eq!(state.type_into_terminal("t", b"x".to_vec()), Typed::Refused);
        assert!(state.problem.is_some());
    }
}
