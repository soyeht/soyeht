//! What Soyeht adds to a production `session/new` or `session/load`: the
//! skill folders the harness loads natively, Claude Code's hooks, and the avatar's persona as the
//! system prompt where the adapter takes one. Probe sessions receive none of them.

use super::state::State;
use crate::errors::Result;
use serde_json::Value;
use std::path::Path;

pub(crate) fn parameters(
    data_directory: &Path,
    state: &State,
    persona: Option<&str>,
    params: Value,
) -> Result<Value> {
    let accepts_directories = state
        .initialized
        .as_ref()
        .is_some_and(|initialized| initialized.additional_directories);
    let params = crate::skills::session_directories(data_directory, accepts_directories, params);
    let params = super::persona::system_prompt(state, persona, params);
    super::hooks::claude_hook_parameters(data_directory, state, params)
}
