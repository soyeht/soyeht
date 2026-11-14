//! The peer rules of ADR 0164 applied to the calls of a pairing and to a device's `hello`.
//!
//! In the person's-code mode `pair.begin` and `pair.prove` are served only to a peer that passed
//! [`pairing_peers`](crate::pairing_peers) and the rules that need the store: the tailnet identity
//! has not changed, and the peer's node is not a device's. Asking the system and Tailscale blocks,
//! so it runs off the Core lock (ADR 0079) and the rules are applied under the lock afterwards.

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    model,
    offload::Offload,
    pairing_peers::{
        Admitted, Identity, ListenerKind, Node, Observed, PairingPeers, Peer, Refusal, SystemPeers,
        admit, in_tailscale_range, observe,
    },
};
use serde_json::Value;
use soyeht_platform::log;
use soyeht_protocol::{HelloParams, method};
use soyeht_store::pairing_nodes;
use std::{net::IpAddr, sync::Arc, time::Duration};

/// How long the system and Tailscale may take to answer; a late answer refuses the call.
const PEER_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the node of a revoked device stays refused.
const REVOKED_NODE_SECONDS: i64 = 24 * 60 * 60;

const LOOKUP_TIMEOUT_MESSAGE: &str = "Soyeht could not ask Tailscale who is connecting";

/// The answers about interfaces and Tailscale: a test's, else the system's.
pub(crate) fn peers(core: &Core) -> Arc<dyn PairingPeers> {
    core.hooks
        .pairing_peers()
        .unwrap_or_else(|| Arc::new(SystemPeers))
}

/// Records where a connection from the network came from, as the engine sees it.
pub(crate) fn note_peer(core: &mut Core, connection: &str, peer: Peer) {
    let seen = peers(core).view_of(peer);
    if let Some(client) = core.clients.get_mut(connection) {
        client.peer = Some(seen);
    }
}

/// Refuses with the reason in words, and writes one line with its kind only. A run of
/// refusals of one kind writes one line, so a flood of calls cannot fill the log.
pub(crate) fn refused(core: &mut Core, refusal: Refusal) -> errors::Error {
    if core.pairing.refusals.fail(refusal.kind()) {
        log::event("engine.pair_refused", &[("kind", refusal.kind())]);
    }
    errors::unauthorized(refusal.message())
}

/// The rules that ask nothing of the system: the connection came through a tailnet listener,
/// from a Tailscale address. Returns the peer when they hold.
pub(crate) fn pair_peer_precheck(core: &mut Core, connection: &str) -> Result<Peer> {
    let client = errors::invariant(
        core.clients.get(connection),
        "Pairing connection disappeared",
    )?;
    let refusal = match client.peer {
        Some(peer) if !client.local && peer.listener == ListenerKind::Tailnet => {
            if in_tailscale_range(peer.address) {
                return Ok(peer);
            }
            Refusal::Address
        }
        _ => Refusal::Listener,
    };
    Err(refused(core, refusal))
}

/// `pair.begin` or `pair.prove` in the person's-code mode: the lookups run with no lock held,
/// and the call is served, or refused, when they are back.
pub(crate) fn pair_offload(
    core: &mut Core,
    connection: &str,
    method: &str,
    params: &Value,
) -> Result<Outcome> {
    let peer = pair_peer_precheck(core, connection)?;
    let peers = peers(core);
    let (connection, method, params) = (connection.to_owned(), method.to_owned(), params.clone());
    Ok(Outcome::Offload(Offload::new(
        LOOKUP_TIMEOUT_MESSAGE,
        PEER_LOOKUP_TIMEOUT,
        move || Ok(observe(peers.as_ref(), peer.address)),
        move |core, observed| {
            // Time has passed: the connection may have changed or gone.
            let now_peer = pair_peer_precheck(core, &connection)?;
            let admitted = admit_pair_peer(core, &method, now_peer, observed)?;
            core.pair_serve(&connection, &method, &params, Some(admitted))
                .map(Outcome::Reply)
        },
    )))
}

/// The rules after the lookups: Tailscale's answer, the tailnet pin and, for `pair.begin`,
/// the table of the nodes of devices.
fn admit_pair_peer(
    core: &mut Core,
    method: &str,
    peer: Peer,
    observed: Result<Observed>,
) -> Result<Admitted> {
    let admitted = observed
        .map_err(|_| Refusal::WhoisUnavailable)
        .and_then(|observed| admit(observed, peer.address))
        .map_err(|refusal| refused(core, refusal))?;
    check_tailnet_pin(core, &admitted.identity)?;
    if method == method::PAIR_BEGIN {
        check_node_table(core, &admitted.node)?;
    }
    core.pairing.refusals.succeed();
    Ok(admitted)
}

/// The first pairing served pins the tailnet and this computer's node key; once either
/// differs, no pairing is served until the person confirms (`confirm_tailnet`).
fn check_tailnet_pin(core: &mut Core, identity: &Identity) -> Result<()> {
    let pinned = pairing_nodes::pinned(&core.store).map_err(errors::storage)?;
    match pinned {
        None => {
            pairing_nodes::pin(
                &core.store,
                &identity.tailnet,
                &identity.node_key,
                core.now_unix(),
            )
            .map_err(errors::storage)?;
            Ok(())
        }
        Some(pinned)
            if pinned.tailnet == identity.tailnet && pinned.node_key == identity.node_key =>
        {
            core.pairing.unconfirmed = None;
            Ok(())
        }
        Some(_) => {
            core.pairing.unconfirmed = Some(identity.clone());
            Err(refused(core, Refusal::TailnetChanged))
        }
    }
}

/// The person confirmed the change a refusal showed: the identity it saw is pinned now.
pub(crate) fn confirm_tailnet(core: &mut Core) -> Result<()> {
    let Some(identity) = core.pairing.unconfirmed.take() else {
        return Ok(());
    };
    pairing_nodes::pin(
        &core.store,
        &identity.tailnet,
        &identity.node_key,
        core.now_unix(),
    )
    .map_err(errors::storage)?;
    log::event("engine.pairing_tailnet_confirmed", &[]);
    Ok(())
}

/// A node a device connected from is not where another pairing begins, and neither is the
/// node of a device revoked less than a day ago.
fn check_node_table(core: &mut Core, node: &Node) -> Result<()> {
    let uses = pairing_nodes::uses_of(&core.store, &node.id).map_err(errors::storage)?;
    let now = core.now_unix();
    for used in uses {
        if !used.revoked {
            return Err(refused(core, Refusal::DeviceNode));
        }
        // A revocation that left no time is treated as recent: any doubt refuses.
        if used
            .revoked_unix
            .is_none_or(|at| at.saturating_add(REVOKED_NODE_SECONDS) > now)
        {
            return Err(refused(core, Refusal::RevokedNode));
        }
    }
    Ok(())
}

/// The answer to `hello`. A paired device on a tailnet connection of the person's-code mode
/// is also looked up, off the lock, for the node it came from; that never blocks the `hello`.
pub(crate) fn hello_request(
    core: &mut Core,
    connection: &str,
    id: &Value,
    params: Value,
) -> Result<Outcome> {
    let Some(address) = device_hello_address(core, connection, &params) else {
        return complete_hello(core, connection, id, &params, None);
    };
    let peers = peers(core);
    let (connection, id) = (connection.to_owned(), id.clone());
    Ok(Outcome::Offload(Offload::new(
        LOOKUP_TIMEOUT_MESSAGE,
        PEER_LOOKUP_TIMEOUT,
        move || Ok(observe(peers.as_ref(), address)),
        move |core, observed| {
            // A lookup that failed or came late leaves the node unknown, not the hello refused.
            let node = observed.map(Observed::other_node).unwrap_or_default();
            complete_hello(core, &connection, &id, &params, node)
        },
    )))
}

/// The peer address to look up for a device's `hello`, when there is one.
fn device_hello_address(core: &Core, connection: &str, params: &Value) -> Option<IpAddr> {
    let client = core.clients.get(connection)?;
    let peer = client.peer?;
    (core.approval().asks_for_code()
        && !client.local
        && peer.listener == ListenerKind::Tailnet
        && in_tailscale_range(peer.address)
        && has_device_token(params))
    .then_some(peer.address)
}

/// Whether a `hello` carries a device token, the only kind that a node is looked up for.
fn has_device_token(params: &Value) -> bool {
    model::typed_params::<HelloParams>(params).is_ok_and(|hello| hello.device_token.is_some())
}

fn complete_hello(
    core: &mut Core,
    connection: &str,
    id: &Value,
    params: &Value,
    node: Option<Node>,
) -> Result<Outcome> {
    let outcome = core
        .hello(connection, params)
        .and_then(|welcome| core.welcome(connection, id, welcome))
        .inspect_err(|error| crate::dispatch::log_refused_handshake(connection, params, error))?;
    if let Some(node) = node {
        record_device_node(core, connection, &node);
    }
    Ok(outcome)
}

/// Keeps the node a device's authenticated `hello` came from. A failure is logged and the
/// `hello` stands.
fn record_device_node(core: &Core, connection: &str, node: &Node) {
    let device = core
        .clients
        .get(connection)
        .and_then(|client| client.actor.as_ref())
        .and_then(|actor| actor.device_id.as_deref());
    if let Some(device) = device
        && let Err(error) = pairing_nodes::record(&core.store, device, &node.id, core.now_unix())
    {
        log::failure("engine.device_node_not_recorded", &errors::storage(error));
    }
}
