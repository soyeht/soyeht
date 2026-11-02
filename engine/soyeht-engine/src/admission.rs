//! Who may hold a connection slot (ADR 0129).
//!
//! The engine serves two kinds of caller from one process: the person's own programs on the
//! local endpoint, and anything that can reach a TCP listener (a tailnet peer, and on the
//! loopback address any program or web page of this machine). The second kind has proved nothing
//! when it connects, and the first handshake read gives it five seconds, so a flood of idle
//! connections is cheap. The two kinds therefore never share slots: a flood of remote
//! connections cannot keep the Mac app, the command line or an agent from reaching the engine,
//! and one remote address cannot take every remote slot from the others.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Connections of the local endpoint at once.
const LOCAL_SLOTS: usize = 128;
/// Connections of every TCP listener at once.
const REMOTE_SLOTS: usize = 128;
/// Connections of one remote address at once: a paired device keeps at most 8 (`hello`), and
/// each of them may be mid-handshake while an old one closes.
const SLOTS_PER_ADDRESS: usize = 16;

/// How many places each remote address holds.
type Held = Arc<Mutex<HashMap<IpAddr, usize>>>;

/// The slots of the local endpoint and of the TCP listeners.
#[derive(Clone)]
pub(crate) struct Admission {
    local: Arc<Semaphore>,
    remote: Arc<Semaphore>,
    addresses: Held,
}

impl Default for Admission {
    fn default() -> Self {
        Self::with_slots(LOCAL_SLOTS, REMOTE_SLOTS)
    }
}

/// A connection's place. Dropping it frees the place.
#[must_use = "the place is free again as soon as this is dropped"]
pub(crate) struct Slot {
    _permit: OwnedSemaphorePermit,
    address: Option<(IpAddr, Held)>,
}

impl Admission {
    fn with_slots(local: usize, remote: usize) -> Self {
        Self {
            local: Arc::new(Semaphore::new(local)),
            remote: Arc::new(Semaphore::new(remote)),
            addresses: Arc::default(),
        }
    }

    /// A place for a connection of the local endpoint, or `None` when all are taken.
    pub(crate) fn local(&self) -> Option<Slot> {
        let Ok(permit) = Arc::clone(&self.local).try_acquire_owned() else {
            return None;
        };
        Some(Slot {
            _permit: permit,
            address: None,
        })
    }

    /// A place for a connection from `peer`, or `None` when the listeners or `peer` have no more.
    pub(crate) fn remote(&self, peer: IpAddr) -> Option<Slot> {
        let peer = peer.to_canonical();
        let Ok(permit) = Arc::clone(&self.remote).try_acquire_owned() else {
            return None;
        };
        // A poisoned count is refused: nothing else says how many this address holds.
        let Ok(mut addresses) = self.addresses.lock() else {
            return None;
        };
        let held = addresses.entry(peer).or_insert(0);
        if *held >= SLOTS_PER_ADDRESS {
            return None;
        }
        *held += 1;
        drop(addresses);
        Some(Slot {
            _permit: permit,
            address: Some((peer, Arc::clone(&self.addresses))),
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let Some((peer, addresses)) = self.address.take() else {
            return;
        };
        // A poisoned count cannot be corrected, and every later `remote` refuses on it too.
        if let Ok(mut addresses) = addresses.lock()
            && let Some(held) = addresses.get_mut(&peer)
        {
            *held -= 1;
            if *held == 0 {
                addresses.remove(&peer);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([100, 64, 0, last])
    }

    #[test]
    fn a_flood_of_remote_connections_leaves_the_local_endpoint_its_places() {
        let admission = Admission::with_slots(2, 3);
        let remote: Vec<_> = (1..=3).filter_map(|n| admission.remote(ip(n))).collect();
        assert_eq!(remote.len(), 3);
        assert!(admission.remote(ip(9)).is_none());
        let local = [admission.local(), admission.local()];
        assert!(local.iter().all(Option::is_some));
        assert!(admission.local().is_none());
    }

    #[test]
    fn one_remote_address_cannot_take_every_remote_place() {
        let admission = Admission::with_slots(1, SLOTS_PER_ADDRESS + 4);
        let greedy: Vec<_> = (0..SLOTS_PER_ADDRESS + 4)
            .filter_map(|_| admission.remote(ip(1)))
            .collect();
        assert_eq!(greedy.len(), SLOTS_PER_ADDRESS);
        assert!(admission.remote(ip(2)).is_some());
        drop(greedy);
        assert!(admission.remote(ip(1)).is_some());
    }

    #[test]
    fn a_place_is_free_again_and_its_address_forgotten_when_the_connection_ends() {
        let admission = Admission::with_slots(1, 1);
        let slot = admission.remote(ip(1));
        assert!(admission.remote(ip(2)).is_none());
        drop(slot);
        assert!(admission.addresses.lock().unwrap().is_empty());
        assert!(admission.remote(ip(2)).is_some());
    }

    #[test]
    fn a_mapped_address_counts_as_the_address_it_maps() {
        let admission = Admission::with_slots(1, SLOTS_PER_ADDRESS * 2);
        let mapped = IpAddr::V6(std::net::Ipv4Addr::new(100, 64, 0, 1).to_ipv6_mapped());
        let held: Vec<_> = (0..SLOTS_PER_ADDRESS * 2)
            .filter_map(|n| admission.remote(if n % 2 == 0 { ip(1) } else { mapped }))
            .collect();
        assert_eq!(held.len(), SLOTS_PER_ADDRESS);
    }
}
