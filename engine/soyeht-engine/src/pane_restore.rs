//! Puts the panes the database holds back in the engine when it starts.

use crate::{
    core::Core,
    errors::{self, Result},
    model::{self, RunningPane, StoredPane},
    screen::Screen,
};
use serde_json::Value;
use std::path::Path;

/// The screen of a pane read from the database. Nobody reads the output of a closed pane, so its
/// segment files are not opened.
fn restored_screen(directory: &Path, stored: &StoredPane) -> Result<Screen> {
    if stored.closed {
        return Ok(Screen::closed());
    }
    Screen::open(
        &directory.join("sessions").join(&stored.pane.pane_id),
        stored.pane.record,
    )
}

impl Core {
    pub(crate) fn restore_panes(&mut self, directory: &Path) -> Result<()> {
        for stored in model::panes(&self.store)? {
            if stored.pane.mode == soyeht_protocol::PaneMode::Agent {
                let state = self.restore_agent(&stored)?;
                self.agents.adopt(stored.pane.pane_id.clone(), state);
            }
            if stored.pane.mode == soyeht_protocol::PaneMode::Terminal
                && let Some(value) = self
                    .store
                    .pane_launch::<Value>(&stored.pane.pane_id)
                    .map_err(errors::storage)?
                && let Some(session) =
                    crate::terminal_harness::TerminalHarnessSession::decode(value)?
            {
                if session.spawn.instance_key != stored.instance_key
                    || session.spawn.generation != stored.pane.generation
                {
                    return Err(errors::internal(
                        "Stored terminal harness session does not match its pane generation",
                    ));
                }
                self.terminal_harnesses
                    .insert(stored.pane.pane_id.clone(), session);
            }
            let screen = restored_screen(directory, &stored)?;
            self.panes
                .insert(stored.pane.pane_id.clone(), RunningPane { stored, screen });
        }
        Ok(())
    }
}
