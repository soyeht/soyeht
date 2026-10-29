//! Merging queued commands that a single request can carry.

use super::{Command, INPUT_CHUNK_BYTES};
use std::sync::mpsc;

/// Merges queued commands that a single request can carry: keystrokes for the same pane go out
/// in one round trip, and of several resizes of one pane only the last matters. A command of
/// another kind stays in order: it is returned in `carry`.
pub fn coalesce(
    first: Command,
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    match first {
        Command::Input { pane_id, bytes } => merge_input(pane_id, bytes, queue, carry),
        Command::Resize {
            pane_id,
            rows,
            cols,
        } => merge_resize(pane_id, (rows, cols), queue, carry),
        other => other,
    }
}

fn merge_input(
    pane_id: String,
    mut bytes: Vec<u8>,
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    while bytes.len() < INPUT_CHUNK_BYTES {
        match queue.try_recv() {
            Ok(Command::Input {
                pane_id: next_pane,
                bytes: more,
            }) if next_pane == pane_id => {
                bytes.extend_from_slice(&more);
            }
            Ok(other) => {
                *carry = Some(other);
                break;
            }
            // ignored: nothing is waiting, or the queue closed: send what was gathered, and the next
            // read of the queue ends the worker.
            Err(_) => break,
        }
    }
    Command::Input { pane_id, bytes }
}

fn merge_resize(
    pane_id: String,
    mut size: (u16, u16),
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    loop {
        match queue.try_recv() {
            Ok(Command::Resize {
                pane_id: next_pane,
                rows,
                cols,
            }) if next_pane == pane_id => {
                size = (rows, cols);
            }
            Ok(other) => {
                *carry = Some(other);
                break;
            }
            // ignored: nothing is waiting, or the queue closed: see `merge_input`.
            Err(_) => break,
        }
    }
    Command::Resize {
        pane_id,
        rows: size.0,
        cols: size.1,
    }
}
