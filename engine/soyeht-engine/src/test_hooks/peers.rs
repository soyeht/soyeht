//! The answers about interfaces and Tailscale that a test gives the engine's pairing rules in
//! place of the system's (ADR 0164, ADR 0091).

use crate::pairing_peers::{Identity, ListenerKind, Node, PairingPeers, Peer};
use std::{
    collections::BTreeMap,
    io,
    net::{IpAddr, Ipv4Addr},
    sync::{Mutex, PoisonError},
};

/// The address the phone of a test engine connects from, as the rules see it.
pub const FIXTURE_PHONE_ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::new(100, 100, 0, 2));
/// The node of that phone.
pub const FIXTURE_PHONE_NODE: &str = "node-phone";
/// This computer's node in the fixture.
pub const FIXTURE_COMPUTER_NODE: &str = "node-computer";
/// The tailnet of the fixture.
pub const FIXTURE_TAILNET: &str = "fixture.example";
/// This computer's node key in the fixture.
pub const FIXTURE_NODE_KEY: &str = "nodekey:fixture";

struct State {
    /// When set, every connection from the network is seen as this one.
    view: Option<Peer>,
    /// The machine's addresses; `None` is a list that cannot be read.
    local: Option<Vec<IpAddr>>,
    /// What Tailscale says each address is; an address with no entry cannot be told.
    nodes: BTreeMap<IpAddr, String>,
    /// This computer's identity; `None` is a daemon that cannot be asked.
    identity: Option<Identity>,
}

/// A machine and a daemon whose answers a test sets.
pub struct FixturePeers {
    state: Mutex<State>,
}

impl FixturePeers {
    fn with(view: Option<Peer>, nodes: BTreeMap<IpAddr, String>) -> Self {
        Self {
            state: Mutex::new(State {
                view,
                local: Some(vec![IpAddr::from([127, 0, 0, 1])]),
                nodes,
                identity: Some(fixture_identity(FIXTURE_TAILNET, FIXTURE_NODE_KEY)),
            }),
        }
    }

    /// A machine that sees every connection as they are, and whose daemon knows no address yet.
    pub fn as_seen() -> Self {
        Self::with(None, BTreeMap::new())
    }

    /// The machine of a test engine, which sees every connection from the network as the phone's:
    /// from a tailnet listener, from [`FIXTURE_PHONE_ADDRESS`], which is [`FIXTURE_PHONE_NODE`].
    pub fn phone() -> Self {
        let peers = Self::with(
            Some(Peer::new(ListenerKind::Tailnet, FIXTURE_PHONE_ADDRESS)),
            BTreeMap::new(),
        );
        peers.set_node(FIXTURE_PHONE_ADDRESS, Some(FIXTURE_PHONE_NODE));
        peers
    }

    /// Says which node Tailscale names for `address`; `None` makes it unable to tell.
    pub fn set_node(&self, address: IpAddr, node: Option<&str>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match node {
            Some(node) => state.nodes.insert(address, node.to_owned()),
            None => state.nodes.remove(&address),
        };
    }

    /// Sets the addresses of the machine's interfaces; `None` is a list that cannot be read.
    pub fn set_local(&self, local: Option<Vec<IpAddr>>) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .local = local;
    }

    /// Sets this computer's identity; `None` is a daemon that cannot be asked.
    pub fn set_identity(&self, identity: Option<Identity>) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .identity = identity;
    }
}

/// The identity of the fixture's computer on `tailnet`, with `node_key`.
pub fn fixture_identity(tailnet: &str, node_key: &str) -> Identity {
    Identity {
        tailnet: tailnet.into(),
        node_key: node_key.into(),
        node: FIXTURE_COMPUTER_NODE.into(),
    }
}

fn unavailable() -> io::Error {
    io::Error::from(io::ErrorKind::ConnectionRefused)
}

impl PairingPeers for FixturePeers {
    fn view_of(&self, peer: Peer) -> Peer {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.view.unwrap_or(peer)
    }

    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.local.clone().ok_or_else(unavailable)
    }

    fn whois(&self, address: IpAddr) -> io::Result<Node> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .nodes
            .get(&address)
            .map(|id| Node { id: id.clone() })
            .ok_or_else(unavailable)
    }

    fn identity(&self) -> io::Result<Identity> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.identity.clone().ok_or_else(unavailable)
    }
}
