//! Delivers alerts in the background so the engine never waits on APNs.
//!
//! Each request is pushed at most once per device; APNs collapses repeats by
//! `apns-collapse-id` as well. Once a request is answered, cancelled or
//! expired, pending retries for it stop.

use super::{
    apns::{ApnsRequest, ApnsResponse, ProviderToken, Transport},
    payload::Alert,
};
use crate::ports::Clock;
use soyeht_platform::log::Line;
use soyeht_protocol::PushEnvironment;
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};

/// Delays before each retry of a network failure, 429 or 5xx.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(120),
];
/// How many request ids are remembered as delivered or settled.
const REMEMBERED_REQUESTS: usize = 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub device_id: String,
    pub token: String,
    pub environment: PushEnvironment,
}

#[derive(Debug)]
pub enum Job {
    Deliver {
        alert: Alert,
        targets: Vec<Target>,
    },
    /// The request was answered, cancelled or expired.
    Settle {
        request_id: String,
    },
}

/// Called with a device id and the token APNs reported as no longer valid.
pub type Forget = Box<dyn Fn(String, String) + Send>;

struct Attempt {
    alert: Alert,
    target: Target,
    retries: usize,
    due: Instant,
}

/// A bounded set that forgets its oldest entries.
#[derive(Default)]
struct Recent {
    order: VecDeque<String>,
    members: HashSet<String>,
}

impl Recent {
    fn insert(&mut self, value: String) -> bool {
        if !self.members.insert(value.clone()) {
            return false;
        }
        self.order.push_back(value);
        if self.order.len() > REMEMBERED_REQUESTS
            && let Some(oldest) = self.order.pop_front()
        {
            self.members.remove(&oldest);
        }
        true
    }

    fn contains(&self, value: &str) -> bool {
        self.members.contains(value)
    }
}

enum Outcome {
    Delivered,
    Retry,
    Dropped,
}

pub struct Worker<T: Transport> {
    pub transport: T,
    pub token: ProviderToken,
    pub clock: Arc<dyn Clock>,
    pub forget: Forget,
}

fn log_failure(status: Option<u16>, reason: &str) {
    Line::new("engine.push_failed")
        .field_opt("status", status)
        .field("reason", reason)
        .emit();
}

impl<T: Transport> Worker<T> {
    pub async fn run(mut self, mut jobs: mpsc::UnboundedReceiver<Job>) {
        let mut settled = Recent::default();
        let mut delivered = Recent::default();
        let mut waiting: Vec<Attempt> = Vec::new();
        loop {
            let next_due = waiting.iter().map(|attempt| attempt.due).min();
            let job = match next_due {
                Some(due) => tokio::select! {
                    job = jobs.recv() => job,
                    () = tokio::time::sleep_until(due) => {
                        let now = Instant::now();
                        let (ready, later): (Vec<_>, Vec<_>) =
                            waiting.drain(..).partition(|attempt| attempt.due <= now);
                        waiting = later;
                        for attempt in ready {
                            if !settled.contains(&attempt.alert.request_id) {
                                self.attempt(attempt, &mut waiting).await;
                            }
                        }
                        continue;
                    }
                },
                None => jobs.recv().await,
            };
            let Some(job) = job else { return };
            match job {
                Job::Settle { request_id } => {
                    waiting.retain(|attempt| attempt.alert.request_id != request_id);
                    settled.insert(request_id);
                }
                Job::Deliver { alert, targets } => {
                    if settled.contains(&alert.request_id) {
                        continue;
                    }
                    for target in targets {
                        let key = format!("{}\n{}", alert.request_id, target.device_id);
                        if !delivered.insert(key) {
                            continue;
                        }
                        let attempt = Attempt {
                            alert: alert.clone(),
                            target,
                            retries: 0,
                            due: Instant::now(),
                        };
                        self.attempt(attempt, &mut waiting).await;
                    }
                }
            }
        }
    }

    async fn attempt(&mut self, mut attempt: Attempt, waiting: &mut Vec<Attempt>) {
        let mut sent = self.send(&attempt).await;
        if sent.as_ref().is_ok_and(|response| {
            response.status == 403 && response.reason.as_deref() == Some("ExpiredProviderToken")
        }) {
            // A new provider token is signed for the one retry.
            self.token.expire();
            sent = self.send(&attempt).await;
        }
        let outcome = match sent {
            Ok(response) => self.judge(&attempt, &response),
            Err(error) => {
                log_failure(None, &error.kind().to_string());
                Outcome::Retry
            }
        };
        if let Outcome::Retry = outcome
            && let Some(delay) = RETRY_DELAYS.get(attempt.retries)
        {
            attempt.retries += 1;
            attempt.due = Instant::now() + *delay;
            waiting.push(attempt);
        }
    }

    async fn send(&mut self, attempt: &Attempt) -> std::io::Result<ApnsResponse> {
        let bearer = self.token.bearer(self.clock.now_unix())?;
        let request = ApnsRequest::alert(
            &attempt.alert,
            &attempt.target.token,
            attempt.target.environment.clone(),
            self.token.topic(),
            &bearer,
        )?;
        self.transport.send(request).await
    }

    fn judge(&self, attempt: &Attempt, response: &ApnsResponse) -> Outcome {
        if response.status == 200 {
            return Outcome::Delivered;
        }
        let reason = response.reason.as_deref().unwrap_or("unknown");
        log_failure(Some(response.status), reason);
        let stale_token =
            response.status == 410 || matches!(reason, "BadDeviceToken" | "DeviceTokenNotForTopic");
        if stale_token {
            (self.forget)(
                attempt.target.device_id.clone(),
                attempt.target.token.clone(),
            );
            return Outcome::Dropped;
        }
        if response.status == 429 || response.status >= 500 {
            Outcome::Retry
        } else {
            Outcome::Dropped
        }
    }
}
