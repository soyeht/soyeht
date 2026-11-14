//! Where a pairing may come from (ADR 0164): the new device is another node of the tailnet.
//!
//! A program of the person's own account can reach the engine's sockets, so it must not be able
//! to be the new device. `pair.begin` and `pair.prove` are served only to a peer that arrived
//! through a tailnet listener, from a Tailscale address that is not any address of this machine,
//! that Tailscale itself says is a node other than this one. The rules here are positive: each one
//! must hold, and any doubt (a list that cannot be read, a daemon that cannot be asked) refuses.
//!
//! The system is asked only through [`PairingPeers`], so a test can say what the interfaces and
//! the daemon answer. [`SystemPeers`] is the shipped answer: `getifaddrs` and the local API of
//! `tailscaled`.

use serde::Deserialize;
use soyeht_protocol::pairing_link::permitted_address;
use std::{io, net::IpAddr};

/// Which of the engine's listeners accepted a connection from the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenerKind {
    /// A listener at an address given on the command line.
    Fixed,
    /// A listener that follows the tailnet interface, bound to it where the system can.
    Tailnet,
}

/// Where a connection from the network came from, as the engine saw it when it accepted it:
/// the listener, and the peer address `TcpStream::peer_addr` gave, in its canonical form (an
/// IPv4 address that arrived as an IPv6 one is IPv4). No header or protocol line is ever read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub listener: ListenerKind,
    pub address: IpAddr,
}

impl Peer {
    pub fn new(listener: ListenerKind, address: IpAddr) -> Self {
        Self {
            listener,
            address: address.to_canonical(),
        }
    }
}

/// A node of the tailnet, by the id Tailscale keeps for it as long as the node exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: String,
}

/// The tailnet this computer is in and its own node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub tailnet: String,
    pub node_key: String,
    pub node: String,
}

/// What the engine asks of the system about a peer.
pub trait PairingPeers: Send + Sync {
    /// How a connection is seen. The system sees it as it is; a test may say that a connection
    /// from loopback came through a tailnet listener from a tailnet address.
    fn view_of(&self, peer: Peer) -> Peer {
        peer
    }

    /// Every address of every interface of this machine, read now.
    fn local_addresses(&self) -> io::Result<Vec<IpAddr>>;

    /// The node `address` belongs to, according to the Tailscale daemon.
    fn whois(&self, address: IpAddr) -> io::Result<Node>;

    /// The tailnet and node of this computer, according to the Tailscale daemon.
    fn identity(&self) -> io::Result<Identity>;
}

/// The system's answers.
pub struct SystemPeers;

impl PairingPeers for SystemPeers {
    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        crate::network::local_addresses()
    }

    fn whois(&self, address: IpAddr) -> io::Result<Node> {
        let body = soyeht_platform::tailscale::get(&format!("/localapi/v0/whois?addr={address}"))?;
        parse_whois(&body, address)
    }

    fn identity(&self) -> io::Result<Identity> {
        let body = soyeht_platform::tailscale::get("/localapi/v0/status?peers=false")?;
        parse_status(&body)
    }
}

fn invalid() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}

#[derive(Deserialize)]
struct WhoisAnswer {
    #[serde(rename = "Node")]
    node: WhoisNode,
}

#[derive(Deserialize)]
struct WhoisNode {
    #[serde(rename = "StableID")]
    stable_id: String,
    #[serde(rename = "Addresses", default)]
    addresses: Vec<String>,
}

/// The node of a `whois` answer. A node whose id is empty, or whose addresses do not include
/// `address`, is not the answer to the question.
fn parse_whois(body: &[u8], address: IpAddr) -> io::Result<Node> {
    let answer: WhoisAnswer = serde_json::from_slice(body).map_err(|_| invalid())?;
    let node = answer.node;
    let names_address = |entry: &String| {
        entry
            .split('/')
            .next()
            .and_then(|text| text.parse::<IpAddr>().ok())
            .is_some_and(|found| found.to_canonical() == address)
    };
    if node.stable_id.is_empty()
        || !node.addresses.is_empty() && !node.addresses.iter().any(names_address)
    {
        return Err(invalid());
    }
    Ok(Node { id: node.stable_id })
}

#[derive(Deserialize)]
struct StatusAnswer {
    #[serde(rename = "Self")]
    this: StatusNode,
    #[serde(rename = "CurrentTailnet")]
    tailnet: StatusTailnet,
}

#[derive(Deserialize)]
struct StatusNode {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "PublicKey")]
    public_key: String,
}

#[derive(Deserialize)]
struct StatusTailnet {
    #[serde(rename = "Name")]
    name: String,
}

fn parse_status(body: &[u8]) -> io::Result<Identity> {
    let status: StatusAnswer = serde_json::from_slice(body).map_err(|_| invalid())?;
    let identity = Identity {
        tailnet: status.tailnet.name,
        node_key: status.this.public_key,
        node: status.this.id,
    };
    if identity.tailnet.is_empty() || identity.node_key.is_empty() || identity.node.is_empty() {
        return Err(invalid());
    }
    Ok(identity)
}

/// Why a pairing call was refused. The kind is what the log says, and never an address or a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The connection did not come through a tailnet listener.
    Listener,
    /// The peer is not at a Tailscale address.
    Address,
    /// The peer's address is one of this machine's, or the machine's addresses cannot be listed.
    LocalAddress,
    /// Tailscale could not be asked who the peer is.
    WhoisUnavailable,
    /// Tailscale says the peer is this computer.
    SameNode,
    /// The request is proved from another node than the one that began it.
    NodeChanged,
    /// The peer's node belongs to a paired device.
    DeviceNode,
    /// The peer's node belonged to a device revoked less than a day ago.
    RevokedNode,
    /// The tailnet or this computer's node is not what the first pairing pinned.
    TailnetChanged,
}

impl Refusal {
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::Listener => "listener",
            Self::Address => "address",
            Self::LocalAddress => "local_address",
            Self::WhoisUnavailable => "whois_unavailable",
            Self::SameNode => "same_node",
            Self::NodeChanged => "node_changed",
            Self::DeviceNode => "device_node",
            Self::RevokedNode => "revoked_node",
            Self::TailnetChanged => "tailnet_changed",
        }
    }

    /// What the device reads: why, in words, without an address or a name.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Listener => {
                "A device pairs from another device, through the tailnet, not from this computer. A computer with Tailscale in userspace mode, `tailscale serve` or Funnel cannot pair a phone"
            }
            Self::Address => "A device pairs from its Tailscale address",
            Self::LocalAddress => {
                "A device pairs from another device, not from this computer, or Soyeht could not list this computer's addresses"
            }
            Self::WhoisUnavailable => {
                "Soyeht could not ask Tailscale who is connecting, so it will not pair"
            }
            Self::SameNode => "A device pairs from another device, not from this computer",
            Self::NodeChanged => "Pairing continues from the device that began it",
            Self::DeviceNode => "A device that is already paired cannot pair another from itself",
            Self::RevokedNode => {
                "This device's access was revoked less than a day ago, so it cannot pair yet"
            }
            Self::TailnetChanged => {
                "The tailnet of this computer changed since it first paired a device: confirm it in Soyeht on the computer"
            }
        }
    }
}

/// Whether `address` is one of Tailscale's: `100.64.0.0/10` or `fd7a:115c:a1e0::/48`.
pub(crate) fn in_tailscale_range(address: IpAddr) -> bool {
    !address.is_loopback() && permitted_address(address)
}

/// What the system said about a peer, asked with no lock held. `None` is "could not be told".
pub(crate) struct Observed {
    local: Option<Vec<IpAddr>>,
    node: Option<Node>,
    identity: Option<Identity>,
}

/// Asks the system about `peer`: this machine's addresses now, the node Tailscale names for it,
/// and this computer's own identity. It blocks, so it runs off the Core lock.
pub(crate) fn observe(peers: &dyn PairingPeers, peer: IpAddr) -> Observed {
    Observed {
        local: told(peers.local_addresses()),
        node: told(peers.whois(peer)),
        identity: told(peers.identity()),
    }
}

/// An answer, or `None` for "could not be told".
fn told<T>(answer: io::Result<T>) -> Option<T> {
    // ignored: an answer that could not be had is "could not be told", which `admit` refuses.
    answer.ok()
}

impl Observed {
    /// The node of the peer, when Tailscale named one and it is not this computer's own.
    pub(crate) fn other_node(self) -> Option<Node> {
        let identity = self.identity?;
        self.node.filter(|node| node.id != identity.node)
    }
}

/// A peer that passed every rule that needs no store.
pub(crate) struct Admitted {
    pub(crate) node: Node,
    pub(crate) identity: Identity,
}

/// The rules of the address and of Tailscale's answer, in the order of ADR 0164.
pub(crate) fn admit(observed: Observed, peer: IpAddr) -> Result<Admitted, Refusal> {
    let local = observed.local.ok_or(Refusal::LocalAddress)?;
    if local
        .iter()
        .any(|address| address.to_canonical() == peer.to_canonical())
    {
        return Err(Refusal::LocalAddress);
    }
    let (Some(node), Some(identity)) = (observed.node, observed.identity) else {
        return Err(Refusal::WhoisUnavailable);
    };
    if node.id == identity.node {
        return Err(Refusal::SameNode);
    }
    Ok(Admitted { node, identity })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn observed(local: &[&str], node: Option<&str>, this: Option<&str>) -> Observed {
        Observed {
            local: Some(local.iter().map(|text| ip(text)).collect()),
            node: node.map(|id| Node { id: id.into() }),
            identity: this.map(|node| Identity {
                tailnet: "tail.example".into(),
                node_key: "nodekey:1".into(),
                node: node.into(),
            }),
        }
    }

    #[test]
    fn only_a_tailscale_address_is_in_range() {
        for inside in [
            "100.64.0.1",
            "100.100.1.2",
            "100.127.255.255",
            "fd7a:115c:a1e0::1",
            "fd7a:115c:a1e0:ab12:1:2:3:4",
        ] {
            assert!(in_tailscale_range(ip(inside)), "{inside}");
        }
        for outside in [
            "127.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "169.254.1.2",
            "fe80::1",
            "172.17.0.2",
            "10.0.0.1",
            "192.168.1.5",
            "100.63.255.255",
            "100.128.0.1",
            "fd7a:115c:a1e1::1",
            "0.0.0.0",
            "8.8.8.8",
        ] {
            assert!(!in_tailscale_range(ip(outside)), "{outside}");
        }
        // A mapped address is read in its canonical form before the range is asked.
        assert!(!in_tailscale_range(ip("::ffff:127.0.0.1").to_canonical()));
        assert!(in_tailscale_range(ip("::ffff:100.64.0.9").to_canonical()));
    }

    #[test]
    fn a_peer_is_canonical_when_it_is_made() {
        let peer = Peer::new(ListenerKind::Tailnet, ip("::ffff:100.64.0.9"));
        assert_eq!(peer.address, ip("100.64.0.9"));
    }

    #[test]
    fn a_peer_that_is_another_node_and_not_this_machine_s_address_is_admitted() {
        let admitted = admit(
            observed(
                &["127.0.0.1", "100.64.0.1"],
                Some("n-phone"),
                Some("n-this"),
            ),
            ip("100.64.0.9"),
        );
        assert_eq!(admitted.unwrap().node.id, "n-phone");
    }

    #[test]
    fn the_rules_refuse_in_the_order_of_the_adr_and_on_any_doubt() {
        let peer = ip("100.64.0.9");
        // One of this machine's own addresses, even a tailnet one.
        let refused = admit(
            observed(&["100.64.0.9"], Some("n-phone"), Some("n-this")),
            peer,
        );
        assert_eq!(refused.err(), Some(Refusal::LocalAddress));
        // The mapped form of the same address is the same address.
        let refused = admit(
            observed(&["::ffff:100.64.0.9"], Some("n-phone"), Some("n-this")),
            peer,
        );
        assert_eq!(refused.err(), Some(Refusal::LocalAddress));
        // The list of addresses could not be read.
        let mut unread = observed(&[], Some("n-phone"), Some("n-this"));
        unread.local = None;
        assert_eq!(admit(unread, peer).err(), Some(Refusal::LocalAddress));
        // Tailscale could not say who the peer is, or who this computer is.
        for (node, this) in [
            (None, Some("n-this")),
            (Some("n-phone"), None),
            (None, None),
        ] {
            assert_eq!(
                admit(observed(&[], node, this), peer).err(),
                Some(Refusal::WhoisUnavailable)
            );
        }
        // Tailscale says the peer is this computer.
        assert_eq!(
            admit(observed(&[], Some("n-this"), Some("n-this")), peer).err(),
            Some(Refusal::SameNode)
        );
    }

    #[test]
    fn a_whois_answer_names_a_stable_node_for_the_address_asked() {
        let answer = br#"{"Node":{"ID":7,"StableID":"nABC123","Name":"phone.tail.ts.net.",
            "Addresses":["100.64.0.9/32","fd7a:115c:a1e0::9/128"]},"UserProfile":{}}"#;
        assert_eq!(parse_whois(answer, ip("100.64.0.9")).unwrap().id, "nABC123");
        assert_eq!(
            parse_whois(answer, ip("fd7a:115c:a1e0::9")).unwrap().id,
            "nABC123"
        );
        // An answer for another address, an empty id or a body that is not an answer.
        for bad in [
            &br#"{"Node":{"StableID":"nABC123","Addresses":["100.64.0.10/32"]}}"#[..],
            br#"{"Node":{"StableID":"","Addresses":["100.64.0.9/32"]}}"#,
            br#"{"Node":{"ID":7}}"#,
            br#"{"Node":null}"#,
            br#"{}"#,
            b"not json",
        ] {
            assert!(parse_whois(bad, ip("100.64.0.9")).is_err());
        }
        // A daemon that lists no addresses is believed about the address it was asked for.
        let bare = br#"{"Node":{"StableID":"nABC123"}}"#;
        assert_eq!(parse_whois(bare, ip("100.64.0.9")).unwrap().id, "nABC123");
    }

    #[test]
    fn a_status_answer_names_the_tailnet_and_this_node() {
        let answer = br#"{"Version":"1.80.0","BackendState":"Running",
            "Self":{"ID":"nSELF1","PublicKey":"nodekey:abc","HostName":"box"},
            "CurrentTailnet":{"Name":"owner@example.com","MagicDNSSuffix":"tail.ts.net"}}"#;
        assert_eq!(
            parse_status(answer).unwrap(),
            Identity {
                tailnet: "owner@example.com".into(),
                node_key: "nodekey:abc".into(),
                node: "nSELF1".into()
            }
        );
        for bad in [
            &br#"{"Self":{"ID":"n","PublicKey":"k"}}"#[..],
            br#"{"Self":{"ID":"","PublicKey":"k"},"CurrentTailnet":{"Name":"t"}}"#,
            br#"{"Self":{"ID":"n","PublicKey":"k"},"CurrentTailnet":{"Name":""}}"#,
            b"{}",
        ] {
            assert!(parse_status(bad).is_err());
        }
    }

    #[test]
    fn a_refusal_names_a_kind_and_no_address() {
        for (refusal, kind) in [
            (Refusal::Listener, "listener"),
            (Refusal::Address, "address"),
            (Refusal::LocalAddress, "local_address"),
            (Refusal::WhoisUnavailable, "whois_unavailable"),
            (Refusal::SameNode, "same_node"),
            (Refusal::NodeChanged, "node_changed"),
            (Refusal::DeviceNode, "device_node"),
            (Refusal::RevokedNode, "revoked_node"),
            (Refusal::TailnetChanged, "tailnet_changed"),
        ] {
            assert_eq!(refusal.kind(), kind);
            assert!(!refusal.message().is_empty());
        }
    }
}
