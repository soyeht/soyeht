//! How long the client waits before it connects to the engine again, as the Mac does
//! (`ReconnectBackoff`; the shared case is `contracts/vectors/reconnect.json`). A refusal that
//! repeats is not retried every second forever: the wait doubles up to a ceiling, and a
//! connection that was established starts it over.

use soyeht_protocol::client_policy::{RECONNECT_CEILING_MS, RECONNECT_DELAY_MS};
use std::time::Duration;

const FIRST: Duration = Duration::from_millis(RECONNECT_DELAY_MS);
const CEILING: Duration = Duration::from_millis(RECONNECT_CEILING_MS);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Backoff {
    delay: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { delay: FIRST }
    }
}

impl Backoff {
    /// The wait before the next attempt; the one after it is twice as long, up to the ceiling.
    pub(crate) fn next(&mut self) -> Duration {
        let wait = self.delay;
        self.delay = (self.delay * 2).min(CEILING);
        wait
    }

    pub(crate) fn reset(&mut self) {
        self.delay = FIRST;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::vector;

    fn millis(value: &serde_json::Value) -> Vec<u128> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|wait| u128::from(wait.as_u64().unwrap()))
            .collect()
    }

    #[test]
    fn the_wait_doubles_to_a_ceiling_and_a_connection_starts_it_over() {
        let vector = vector("reconnect");
        let mut backoff = Backoff::default();
        let waits: Vec<u128> = (0..millis(&vector["waitsMs"]).len())
            .map(|_| backoff.next().as_millis())
            .collect();
        assert_eq!(waits, millis(&vector["waitsMs"]));
        backoff.reset();
        assert_eq!(
            backoff.next().as_millis(),
            u128::from(vector["afterResetMs"].as_u64().unwrap())
        );
    }
}
