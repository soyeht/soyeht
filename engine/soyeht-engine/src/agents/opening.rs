//! What Soyeht adds to a production `session/new` or `session/load`: the
//! skill folders the harness loads natively, and Claude Code's hooks.
//! Probe sessions receive neither.

use super::state::State;
use crate::errors::Result;
use serde_json::Value;
use std::path::Path;

pub(crate) fn parameters(data_directory: &Path, state: &State, params: Value) -> Result<Value> {
    let accepts_directories = state
        .initialized
        .as_ref()
        .is_some_and(|initialized| initialized.additional_directories);
    let params = crate::skills::session_directories(data_directory, accepts_directories, params);
    super::hooks::claude_hook_parameters(data_directory, state, params)
}
