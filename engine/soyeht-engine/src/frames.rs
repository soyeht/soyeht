//! Reading one connection's frames.

use crate::{
    core_lock::{self, Shared},
    errors,
    outbox::CloseReason,
    transport,
};
use soyeht_protocol::frame::Frame;
use std::time::Duration;

/// Reads the next frame. Until the handshake completes it is bounded by a
/// deadline; afterwards (`authenticated`, which never reverts) the read takes
/// no Core lock, so a signed-in client's frames cost no extra hop.
pub(crate) async fn next_frame(
    core: &Shared,
    id: &str,
    reader: &mut transport::Reader,
    authenticated: &mut bool,
) -> Result<Frame, CloseReason> {
    let connection = id.to_owned();
    let wait = if *authenticated {
        None
    } else {
        handshake_wait(core, connection).await?
    };
    *authenticated |= wait.is_none();
    let result = if let Some(wait) = wait {
        tokio::time::timeout(wait, reader.next())
            .await
            .map_err(|_| CloseReason::HandshakeTimeout)?
    } else {
        reader.next().await
    };
    match result {
        Ok(Some(frame)) => Ok(frame),
        Ok(None) => Err(CloseReason::PeerClosed),
        Err(_) => Err(CloseReason::ReadError),
    }
}

/// The read deadline while a connection has not finished its handshake.
async fn handshake_wait(
    core: &Shared,
    connection: String,
) -> Result<Option<Duration>, CloseReason> {
    core_lock::with(core, "read_wait", move |core| {
        let client = core
            .clients
            .get(&connection)
            .ok_or_else(|| errors::internal("Connection disappeared"))?;
        Ok(if client.actor.is_some() {
            None
        } else if client.pair_request.is_some() {
            Some(Duration::from_secs(600))
        } else {
            Some(Duration::from_secs(5))
        })
    })
    .await
    .map_err(|_| CloseReason::WorkerFailed)
}
