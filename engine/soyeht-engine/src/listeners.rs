//! The loops that accept the connections of the engine's listeners and hand each to
//! [`connection`](crate::server).

use crate::{
    accept_loop::Failures,
    admission::Admission,
    core_lock::Shared,
    pairing_peers::{ListenerKind, Peer},
    server::{CatalogJob, connection},
    transport, ui_attestation,
};
use soyeht_platform::local::asynchronous::{Listener, Stream, same_user};
use soyeht_protocol::profile::Profile;
use std::{io, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::mpsc};

fn local_peer_audit_token(stream: &Stream) -> Option<ui_attestation::PeerAuditToken> {
    same_user(stream)
        .then(|| ui_attestation::peer_audit_token(stream))
        .flatten()
}

/// What every accepted connection shares.
#[derive(Clone)]
pub(crate) struct Accept {
    pub(crate) profile: &'static Profile,
    pub(crate) core: Shared,
    pub(crate) admission: Admission,
    pub(crate) catalog_tx: mpsc::Sender<CatalogJob>,
}

/// Accepts the connections of this user on the local endpoint, for as long as the engine runs.
pub(crate) async fn serve_local(mut local: Listener, accept: Accept) -> crate::errors::Result<()> {
    let Accept {
        profile,
        core,
        admission,
        catalog_tx,
    } = accept;
    let mut failures = Failures::new("local");
    loop {
        let stream = failures.next(&mut local).await?;
        let Some(peer_audit_token) = local_peer_audit_token(&stream) else {
            continue;
        };
        let Some(slot) = admission.local() else {
            continue;
        };
        let core = Arc::clone(&core);
        let catalog_tx = catalog_tx.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let signed_ui = tokio::task::spawn_blocking(move || {
                ui_attestation::signed_ui_peer(peer_audit_token, profile)
            })
            .await
            .unwrap_or(false);
            let (reader, writer) = transport::local(stream);
            connection(core, catalog_tx, true, signed_ui, None, reader, writer).await;
        });
    }
}

/// Accepts the connections of a TCP listener for as long as the engine runs. A failed `accept` is
/// logged and waited out; a listener that cannot recover ends with its error (`accept_loop`).
pub(crate) async fn serve_tcp(
    mut listener: TcpListener,
    accept: Accept,
    kind: ListenerKind,
) -> io::Result<()> {
    let mut failures = Failures::new(
        listener
            .local_addr()
            .map_or_else(|_| "tcp".to_owned(), |address| address.to_string()),
    );
    loop {
        let (stream, peer) = failures.next(&mut listener).await?;
        let Some(slot) = accept.admission.remote(peer.ip()) else {
            continue;
        };
        let (core, catalog_tx) = (Arc::clone(&accept.core), accept.catalog_tx.clone());
        // What `peer_addr` says, and nothing a client sends: no header or protocol line names it.
        let peer = Peer::new(kind, peer.ip());
        tokio::spawn(async move {
            let _slot = slot;
            let websocket = transport::websocket(stream);
            if let Ok(Ok((reader, writer))) =
                tokio::time::timeout(Duration::from_secs(5), websocket).await
            {
                connection(core, catalog_tx, false, false, Some(peer), reader, writer).await;
            }
        });
    }
}
