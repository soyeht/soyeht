#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use rusqlite::params;
use soyeht_store::agents;
use support::Profile;

#[test]
fn a_stored_readiness_with_a_state_or_reason_nobody_defined_is_an_error_not_a_guess() {
    let profile = Profile::new();
    let store = profile.store();
    for (state, reason) in [("not_a_state", None), ("ready", Some("not_a_reason"))] {
        store
            .connection()
            .execute(
                "INSERT OR REPLACE INTO harness_readiness(harness_id,state,reason,last_ready_ms,updated_ms) VALUES ('h',?1,?2,NULL,1)",
                params![state, reason],
            )
            .unwrap();
        assert!(
            agents::harness_readiness(&store, "h").is_err(),
            "{state} {reason:?}"
        );
    }
}
