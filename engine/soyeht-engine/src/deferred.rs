//! Outcomes that finish outside the engine lock: the dispatcher prepares them under the lock,
//! the slow part runs on a blocking thread, and the result is committed under the lock again.

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Error},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// The kinds of outcome whose slow part runs outside the engine lock.
pub enum Deferred {
    WritingSectionList(crate::writing::PreparedWritingSectionList),
    UsageRefresh(crate::usage::PreparedUsageRefresh),
}

impl From<crate::writing::PreparedWritingSectionList> for Outcome {
    fn from(prepared: crate::writing::PreparedWritingSectionList) -> Self {
        Self::Deferred(Deferred::WritingSectionList(prepared))
    }
}

impl From<crate::usage::PreparedUsageRefresh> for Outcome {
    fn from(prepared: crate::usage::PreparedUsageRefresh) -> Self {
        Self::Deferred(Deferred::UsageRefresh(prepared))
    }
}

type Resolved = std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError>;

const WRITING_SECTION_LIST_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn resolve(core: Arc<Mutex<Core>>, outcome: Resolved) -> Resolved {
    match outcome {
        Ok(Ok(Outcome::Deferred(Deferred::UsageRefresh(prepared)))) => {
            usage_refresh(core, prepared).await
        }
        Ok(Ok(Outcome::Deferred(Deferred::WritingSectionList(prepared)))) => {
            let scanned = match spawn_blocking_with_timeout(
                WRITING_SECTION_LIST_TIMEOUT,
                move || prepared.scan(),
                "Writing section listing timed out",
            )
            .await?
            {
                Ok(scanned) => scanned,
                Err(error) => return Ok(Err(error)),
            };
            tokio::task::spawn_blocking(move || {
                let core = core.lock().map_err(|_| Error::EngineStatePoisoned)?;
                scanned.finish(&core).map(Outcome::Reply)
            })
            .await
        }
        other => other,
    }
}

/// Reads a click's usage sources without the engine lock, then commits under it. The sources
/// bound their own waits, so the read always returns and releases its providers.
async fn usage_refresh(
    core: Arc<Mutex<Core>>,
    prepared: crate::usage::PreparedUsageRefresh,
) -> Resolved {
    let (finish, batch) = tokio::task::spawn_blocking(move || prepared.read()).await?;
    tokio::task::spawn_blocking(move || {
        let mut core = core.lock().map_err(|_| Error::EngineStatePoisoned)?;
        core.usage_refresh_finish(finish, batch).map(Outcome::Reply)
    })
    .await
}

pub(crate) async fn spawn_blocking_with_timeout<T, F>(
    timeout: Duration,
    operation: F,
    timeout_message: &'static str,
) -> std::result::Result<crate::errors::Result<T>, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> crate::errors::Result<T> + Send + 'static,
{
    match tokio::time::timeout(timeout, tokio::task::spawn_blocking(operation)).await {
        Ok(result) => result,
        Err(_) => Ok(Err(errors::internal(timeout_message))),
    }
}
