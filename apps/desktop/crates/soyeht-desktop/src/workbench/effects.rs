//! Carrying out what the replica asks for, and telling the engine about a terminal's size.

use super::Workbench;
use crate::destination::Destination;
use soyeht_client::{Command, Effect};

impl Workbench {
    /// Carries out one thing the replica asks for.
    pub(crate) fn run_effect(&mut self, effect: Effect) {
        match effect {
            Effect::SubscribeTerminal(pane_id) => {
                self.send(Command::Subscribe { pane_id });
            }
            Effect::Resync => self.engine.resync(),
            // A pane's queue of waiting messages is loaded with its transcript.
            Effect::LoadTranscript(pane_id) => {
                self.send(Command::LoadQueue {
                    pane_id: pane_id.clone(),
                });
                self.send(Command::LoadTranscript { pane_id });
            }
            Effect::LoadUsage => {
                self.send(Command::LoadUsage);
            }
            Effect::LoadCommands(pane_id) => {
                self.send(Command::LoadCommands { pane_id });
            }
            Effect::LoadSkills(workspace_id) => {
                self.send(Command::LoadSkills { workspace_id });
            }
            Effect::LoadHarnesses => {
                self.send(Command::LoadHarnesses { rescan: false });
            }
            Effect::LoadReadiness => {
                self.send(Command::LoadReadiness);
            }
            Effect::LoadTask {
                workspace_id,
                task_id,
            } => {
                self.send(Command::LoadTask {
                    workspace_id,
                    task_id,
                });
            }
            Effect::StoreLayout {
                workspace_id,
                layout,
            } => {
                // A queue that is full refuses it the way an engine does: the layout goes back.
                self.run_effect(Effect::Send(Command::SetLayout {
                    workspace_id,
                    layout,
                }));
            }
            // The pane the person opened takes the keyboard, as a new terminal does on the Mac.
            Effect::FocusPane(pane_id) => {
                // A pane a task was dispatched to is in the agents' place.
                if !self.destination.is_pane_grid() {
                    self.destination = Destination::Agents;
                }
                self.state.focus_pane(&pane_id);
                self.announce_focus();
                self.stage.reveal = Some(pane_id.clone());
                self.focus_request = Some(pane_id);
            }
            Effect::Send(command) => {
                // A command the queue refuses never gets an answer: the replica is told, so that
                // what waits for it is not stuck.
                let failure = command.failure(&soyeht_client::Error::Busy);
                if !self.send(command)
                    && let Some(event) = failure
                {
                    let effects = self.state.apply(event);
                    self.run_effects(effects);
                }
            }
        }
    }

    /// Called while painting a terminal: fits its screen to the room it has and tells the
    /// engine when the size really changed.
    pub fn fit_terminal(&mut self, pane_id: &str, rows: u16, cols: u16) {
        if let Some((rows, cols)) = self.state.fit_terminal(pane_id, rows, cols) {
            self.send(Command::Resize {
                pane_id: pane_id.to_owned(),
                rows,
                cols,
            });
        }
    }
}
