//! What a test that counts the engine's work sees of it (test hooks only, ADR 0091).
//!
//! With `SOYEHT_TEST_MEASURE_COST` set, the test engine writes one log line for every hold of the
//! Core lock (`test.lock_held`) and one for every request it makes to the supervisor
//! (`test.supervisor_request`). Each carries `commits`, the commits every store of the process has
//! made so far, so a test that reads the log between two points gets the commits, the requests and
//! the lock time of what it did in between.

use crate::ports::SupervisorPort;
use soyeht_platform::log::Line;
use soyeht_ptyd::wire::{Request, Response};
use std::time::{Duration, Instant};

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub(crate) fn lock_held(purpose: &str, method: &str, held: Duration, waited: Duration) {
    Line::new("test.lock_held")
        .field("purpose", purpose)
        .field("method", method)
        .field("heldUs", micros(held))
        .field("waitedUs", micros(waited))
        .field("commits", soyeht_store::test_support::commits_made())
        .emit();
}

/// The name of a request on the supervisor's wire, which is the serde tag of the variant.
fn kind(request: &Request) -> String {
    serde_json::to_value(request)
        .ok()
        .and_then(|value| value["method"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// A link that writes a line for each request and how long its round trip took.
pub(crate) struct Counting(pub(crate) Box<dyn SupervisorPort>);

impl SupervisorPort for Counting {
    fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
        let kind = kind(&request);
        let started = Instant::now();
        let response = self.0.request(request);
        Line::new("test.supervisor_request")
            .field("kind", kind)
            .field("us", micros(started.elapsed()))
            .field("commits", soyeht_store::test_support::commits_made())
            .emit();
        response
    }

    fn supervisor_id(&self) -> &str {
        self.0.supervisor_id()
    }

    fn supervisor_generation(&self) -> u32 {
        self.0.supervisor_generation()
    }
}
