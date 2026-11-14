//! The system's answers about interfaces and Tailscale (ADR 0164), with the limits that keep a
//! peer from using them against the engine.
//!
//! Every pairing call and every device `hello` from the tailnet may ask the machine for its
//! addresses and the daemon for a node, and both block. So the answers about this machine (its
//! addresses, its own identity) are kept for a couple of seconds, at most a few lookups run at
//! once and the rest are refused at once, and a daemon that did not answer is not asked again for
//! a few seconds. A refused or late lookup is "could not be told", which pairing refuses and a
//! `hello` does without.

use crate::pairing_peers::{Identity, Node, PairingPeers, parse_status, parse_whois};
use std::{
    io,
    net::IpAddr,
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// How long the addresses of this machine and its own identity are believed.
const KEPT_FOR: Duration = Duration::from_secs(2);
/// How long a daemon that did not answer is left alone.
const LEFT_ALONE_FOR: Duration = Duration::from_secs(5);
/// The most lookups that run at once.
const MOST_AT_ONCE: usize = 4;

type Daemon = Box<dyn Fn(&str) -> io::Result<Vec<u8>> + Send + Sync>;
type Interfaces = Box<dyn Fn() -> io::Result<Vec<IpAddr>> + Send + Sync>;

#[derive(Default)]
struct Memory {
    addresses: Option<(Instant, Vec<IpAddr>)>,
    identity: Option<(Instant, Identity)>,
    /// When the daemon last failed to answer at all.
    silent_since: Option<Instant>,
}

/// The system's answers: `getifaddrs` and the local API of `tailscaled`.
pub struct SystemPeers {
    daemon: Daemon,
    interfaces: Interfaces,
    kept_for: Duration,
    left_alone_for: Duration,
    running: AtomicUsize,
    memory: Mutex<Memory>,
}

/// A running lookup, which gives its place back when it ends.
struct Running<'a>(&'a AtomicUsize);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl SystemPeers {
    /// The one that every call of the engine shares, so the limits are the whole engine's.
    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<SystemPeers>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| {
            Arc::new(Self::over(
                Box::new(soyeht_platform::tailscale::get),
                Box::new(crate::network::local_addresses),
                KEPT_FOR,
                LEFT_ALONE_FOR,
            ))
        }))
    }

    fn over(
        daemon: Daemon,
        interfaces: Interfaces,
        kept_for: Duration,
        left_alone_for: Duration,
    ) -> Self {
        Self {
            daemon,
            interfaces,
            kept_for,
            left_alone_for,
            running: AtomicUsize::new(0),
            memory: Mutex::new(Memory::default()),
        }
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, Memory> {
        // Nothing runs while the memory is locked but a copy, so a poisoned one is still whole.
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A place among the lookups that run, or `WouldBlock` when they are all taken.
    fn place(&self) -> io::Result<Running<'_>> {
        if self.running.fetch_add(1, Ordering::AcqRel) >= MOST_AT_ONCE {
            self.running.fetch_sub(1, Ordering::AcqRel);
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        Ok(Running(&self.running))
    }

    /// One request to the daemon, unless it was silent a moment ago or the places are taken.
    fn ask(&self, target: &str) -> io::Result<Vec<u8>> {
        if self
            .memory()
            .silent_since
            .is_some_and(|since| since.elapsed() < self.left_alone_for)
        {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        let _place = self.place()?;
        let answer = (self.daemon)(target);
        // A daemon that is absent, refuses this account or does not answer in time is left alone;
        // one that answered something unusable is not silent.
        let silent = answer.as_ref().err().is_some_and(|error| {
            matches!(
                error.kind(),
                io::ErrorKind::TimedOut
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::NotFound
                    | io::ErrorKind::PermissionDenied
            )
        });
        self.memory().silent_since = silent.then(Instant::now);
        answer
    }
}

impl PairingPeers for SystemPeers {
    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        if let Some((at, addresses)) = &self.memory().addresses
            && at.elapsed() < self.kept_for
        {
            return Ok(addresses.clone());
        }
        let _place = self.place()?;
        let addresses = (self.interfaces)()?;
        self.memory().addresses = Some((Instant::now(), addresses.clone()));
        Ok(addresses)
    }

    fn whois(&self, address: IpAddr) -> io::Result<Node> {
        let body = self.ask(&format!("/localapi/v0/whois?addr={address}"))?;
        parse_whois(&body, address)
    }

    fn identity(&self) -> io::Result<Identity> {
        if let Some((at, identity)) = &self.memory().identity
            && at.elapsed() < self.kept_for
        {
            return Ok(identity.clone());
        }
        let identity = parse_status(&self.ask("/localapi/v0/status?peers=false")?)?;
        self.memory().identity = Some((Instant::now(), identity.clone()));
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, mpsc};

    const STATUS: &[u8] = br#"{"Self":{"ID":"nSELF","PublicKey":"nodekey:k"},
        "CurrentTailnet":{"Name":"t.example"}}"#;
    const WHOIS: &[u8] = br#"{"Node":{"StableID":"nPHONE","Addresses":["100.64.0.9/32"]}}"#;

    fn peer() -> IpAddr {
        "100.64.0.9".parse().unwrap()
    }

    /// Peers over a daemon that answers `reply(target)` and counts what it was asked.
    fn peers(
        reply: impl Fn(&str) -> io::Result<Vec<u8>> + Send + Sync + 'static,
        kept_for: Duration,
        left_alone_for: Duration,
    ) -> (SystemPeers, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let asked = Arc::new(AtomicUsize::new(0));
        let listed = Arc::new(AtomicUsize::new(0));
        let (asked_by, listed_by) = (Arc::clone(&asked), Arc::clone(&listed));
        let peers = SystemPeers::over(
            Box::new(move |target| {
                asked_by.fetch_add(1, Ordering::SeqCst);
                reply(target)
            }),
            Box::new(move || {
                listed_by.fetch_add(1, Ordering::SeqCst);
                Ok(vec!["127.0.0.1".parse().unwrap()])
            }),
            kept_for,
            left_alone_for,
        );
        (peers, asked, listed)
    }

    fn answers(target: &str) -> io::Result<Vec<u8>> {
        Ok(if target.contains("whois") {
            WHOIS
        } else {
            STATUS
        }
        .to_vec())
    }

    #[test]
    fn this_machine_s_addresses_and_identity_are_kept_for_a_moment_and_a_peer_is_not() {
        let (peers, asked, listed) = peers(answers, Duration::from_millis(150), KEPT_FOR);
        for _ in 0..3 {
            assert_eq!(peers.identity().unwrap().node, "nSELF");
            assert_eq!(peers.local_addresses().unwrap().len(), 1);
        }
        assert_eq!(
            (asked.load(Ordering::SeqCst), listed.load(Ordering::SeqCst)),
            (1, 1)
        );
        // Who a peer is, is asked every time.
        peers.whois(peer()).unwrap();
        peers.whois(peer()).unwrap();
        assert_eq!(asked.load(Ordering::SeqCst), 3);
        // Then the memory runs out and both are read again.
        std::thread::sleep(Duration::from_millis(200));
        peers.identity().unwrap();
        peers.local_addresses().unwrap();
        assert_eq!(
            (asked.load(Ordering::SeqCst), listed.load(Ordering::SeqCst)),
            (4, 2)
        );
    }

    #[test]
    fn only_a_few_lookups_run_at_once_and_the_rest_are_refused_at_once() {
        let (release, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let started = Arc::new(AtomicUsize::new(0));
        let started_by = Arc::clone(&started);
        let (peers, asked, _) = peers(
            move |_| {
                started_by.fetch_add(1, Ordering::SeqCst);
                // Held until the test lets go.
                let _ = wait.lock().unwrap().recv();
                Ok(WHOIS.to_vec())
            },
            KEPT_FOR,
            KEPT_FOR,
        );
        let peers = Arc::new(peers);
        let threads: Vec<_> = (0..MOST_AT_ONCE)
            .map(|_| {
                let peers = Arc::clone(&peers);
                std::thread::spawn(move || peers.whois(peer()))
            })
            .collect();
        while started.load(Ordering::SeqCst) < MOST_AT_ONCE {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Every place is taken: another lookup, of any kind, is refused without asking.
        let began = Instant::now();
        assert_eq!(
            peers.whois(peer()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            peers.identity().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(began.elapsed() < Duration::from_millis(500));
        assert_eq!(asked.load(Ordering::SeqCst), MOST_AT_ONCE);
        for _ in &threads {
            release.send(()).unwrap();
        }
        for thread in threads {
            assert!(thread.join().unwrap().is_ok());
        }
        // The places are given back.
        assert_eq!(peers.running.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_daemon_that_did_not_answer_is_left_alone_for_a_while_but_a_bad_answer_is_not_silence() {
        let silent = Arc::new(AtomicBool::new(true));
        let silent_by = Arc::clone(&silent);
        let (peers, asked, _) = peers(
            move |target| {
                if silent_by.load(Ordering::SeqCst) {
                    Err(io::Error::from(io::ErrorKind::TimedOut))
                } else {
                    answers(target)
                }
            },
            KEPT_FOR,
            Duration::from_millis(150),
        );
        assert!(peers.whois(peer()).is_err());
        // Left alone: the next ones are refused without a request.
        for _ in 0..5 {
            assert!(peers.whois(peer()).is_err());
            assert!(peers.identity().is_err());
        }
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        silent.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(peers.whois(peer()).unwrap().id, "nPHONE");
        // A daemon that answered nonsense is asked again at once.
        let (peers, asked, _) = peers_with_garbage();
        assert!(peers.whois(peer()).is_err());
        assert!(peers.whois(peer()).is_err());
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }

    fn peers_with_garbage() -> (SystemPeers, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        peers(|_| Ok(b"nonsense".to_vec()), KEPT_FOR, KEPT_FOR)
    }
}
