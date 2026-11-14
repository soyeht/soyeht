//! Agent steps on a pane that has no running state behind its id.

use crate::{errors, test_panes};
use soyeht_protocol::ErrorKind;

/// A pane id that does not exist fails the log decode with a typed error, where the panes map
/// once indexed and panicked.
#[test]
fn decoding_the_log_of_a_missing_pane_fails_with_an_error() {
    let (mut core, directory) = test_panes::core_with_workspace("workspace");
    let error = core.decode_agent_log("missing").err();
    assert!(error.is_some_and(|error| errors::kind(&error) == ErrorKind::InternalError));
    crate::test_paths::remove_after(core, directory);
}

/// An agent whose pane row went away fails its poll with a typed error instead of panicking.
#[test]
fn polling_an_agent_whose_pane_row_went_away_fails_with_an_error() {
    let (mut core, directory) = test_panes::core_with_workspace("workspace");
    test_panes::insert_agent_pane(&mut core, &directory, "agent-pane", "workspace");
    assert!(core.panes.remove("agent-pane").is_some());
    let error = core.poll_agent("agent-pane").err();
    assert!(error.is_some_and(|error| errors::kind(&error) == ErrorKind::InternalError));
    crate::test_paths::remove_after(core, directory);
}
