//! Push notifications for questions agents ask the person (contract 08 §2.3).
//!
//! A paired device registers its APNs token with `push.register`. When an
//! agent pane publishes `pane.permission_requested` or
//! `pane.elicitation_requested`, every registered device of this Mac gets an
//! alert; the phone answers through the ordinary `pane.permission.answer`.
//! Without `<data dir>/push/apns.json` push stays off after one log line.

pub mod apns;
pub mod payload;
#[cfg(test)]
mod tests;
pub mod worker;

use crate::{
    core::Core,
    errors::{self, Result},
    ports::{Clock, StorePort},
};
use serde_json::{Value, json};
use soyeht_protocol::{PushRegisterParams, Role};
use soyeht_store::database::Event;
use std::{
    path::Path,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::mpsc;
use worker::{Forget, Job, Target, Worker};

/// The engine's handle to the delivery worker; disabled without credentials.
#[derive(Default)]
pub struct Notifier {
    jobs: Option<mpsc::UnboundedSender<Job>>,
}

impl Notifier {
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Runs `worker` on the current Tokio runtime.
    pub fn spawn<T: apns::Transport>(worker: Worker<T>) -> Self {
        let (jobs, receiver) = mpsc::unbounded_channel();
        tokio::spawn(worker.run(receiver));
        Self { jobs: Some(jobs) }
    }

    /// Loads the local APNs credentials and starts delivery over HTTP/2, or
    /// logs once why push stays off.
    pub fn start(data_directory: &Path, clock: Arc<dyn Clock>, forget: Forget) -> Self {
        match apns::Credentials::load(data_directory) {
            Ok(credentials) => {
                eprintln!("{}", json!({"event":"engine.push_enabled"}));
                Self::spawn(Worker {
                    transport: apns::Http2::default(),
                    token: apns::ProviderToken::new(credentials),
                    clock,
                    forget,
                })
            }
            Err(problem) => {
                eprintln!(
                    "{}",
                    json!({"event":"engine.push_disabled","reason":problem.as_str()})
                );
                Self::disabled()
            }
        }
    }

    fn submit(&self, job: Job) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(job);
        }
    }
}

/// Shares the engine state and starts push delivery. A token APNs reports
/// as stale is removed through the shared state, off the async workers.
pub fn share(core: Core, data_directory: &Path) -> Arc<Mutex<Core>> {
    Arc::new_cyclic(|shared: &Weak<Mutex<Core>>| {
        let shared = shared.clone();
        let forget: Forget = Box::new(move |device, token| {
            let shared = shared.clone();
            tokio::task::spawn_blocking(move || {
                let Some(core) = shared.upgrade() else { return };
                let removed = core
                    .lock()
                    .map_err(|_| errors::internal("Engine state lock poisoned"))
                    .and_then(|mut core| core.forget_push_token(&device, &token));
                if removed.is_err() {
                    eprintln!(
                        "{}",
                        json!({"event":"engine.push_failed","reason":"token_not_removed"})
                    );
                }
            });
        });
        let mut core = core;
        core.push = Notifier::start(data_directory, Arc::clone(&core.clock), forget);
        Mutex::new(core)
    })
}

impl Core {
    /// Hands published questions and their answers to the delivery worker.
    /// Push never fails or delays publication.
    pub(crate) fn notify_push(&self, events: &[Event]) {
        if self.push.jobs.is_none() {
            return;
        }
        for event in events {
            let request_id = || event.params["requestId"].as_str().map(str::to_owned);
            match event.method.as_str() {
                "pane.permission_requested" | "pane.elicitation_requested" => {
                    if let Some(job) = self.push_delivery(event) {
                        self.push.submit(job);
                    }
                }
                "pane.permission_answered" | "pane.elicitation_answered" => {
                    if let Some(request_id) = request_id() {
                        self.push.submit(Job::Settle { request_id });
                    }
                }
                _ => {}
            }
        }
    }

    fn push_delivery(&self, event: &Event) -> Option<Job> {
        let pane_id = event.params["paneId"].as_str()?;
        let pane = &self.panes.get(pane_id)?.stored.pane;
        if pane.mode != soyeht_protocol::PaneMode::Agent {
            return None;
        }
        let harness = self.agents.get(pane_id).map_or("Agent", |running| {
            crate::agents::naming::harness_label(&running.state.launch)
        });
        let alert = payload::alert(&payload::Question {
            event: &event.method,
            request: &event.params,
            workspace_id: &pane.workspace_id,
            harness,
            engine_id: &self.store.db_id,
        })?;
        let targets = match self.store.push_registrations() {
            Ok(registrations) => registrations
                .into_iter()
                .map(|registration| Target {
                    device_id: registration.device_id,
                    token: registration.token,
                    environment: registration.environment,
                })
                .collect::<Vec<_>>(),
            Err(_) => {
                eprintln!(
                    "{}",
                    json!({"event":"engine.push_failed","reason":"registrations_unavailable"})
                );
                return None;
            }
        };
        (!targets.is_empty()).then_some(Job::Deliver { alert, targets })
    }

    /// `push.register` and `push.unregister`: only a paired device has a
    /// token, and it manages its own registration.
    pub(crate) fn push_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let actor = self
            .clients
            .get(connection)
            .and_then(|client| client.actor.as_ref())
            .ok_or_else(|| errors::unauthorized("Handshake required"))?;
        let device = match (&actor.role, &actor.device_id) {
            (Role::HumanDevice, Some(device)) => device.clone(),
            _ => {
                return Err(errors::forbidden_role(
                    "Only a paired device registers for push notifications",
                ));
            }
        };
        let register = if method == "push.register" {
            let register = serde_json::from_value::<PushRegisterParams>(params.clone())
                .map_err(|_| errors::invalid("Invalid push registration"))?;
            // The token is the hex form of the bytes APNs gave the device.
            if !register
                .token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(errors::invalid("Push token must be lowercase hexadecimal"));
            }
            Some(register)
        } else {
            None
        };
        let now = self.now_unix();
        Ok(self
            .mutate(connection, method, params, |change| {
                match &register {
                    Some(register) => change.register_push(
                        &device,
                        &register.token,
                        &register.environment,
                        now,
                    )?,
                    None => change.unregister_push(&device)?,
                }
                Ok((json!({"deviceId":device}), vec![]))
            })?
            .result)
    }

    /// Drops a token APNs reported as no longer valid for this app.
    pub fn forget_push_token(&mut self, device_id: &str, token: &str) -> Result<()> {
        StorePort::mutate(
            &mut self.store,
            "engine",
            &uuid::Uuid::new_v4().to_string(),
            "push.forget",
            |change| {
                change.forget_push_token(device_id, token)?;
                Ok((json!({}), vec![]))
            },
        )
        .map_err(errors::storage)?;
        Ok(())
    }
}
