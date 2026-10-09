use crate::{
    core::Core,
    errors::{Error, Result},
};
use serde_json::{Value, json};
use soyeht_protocol::{EngineReachability, EngineReachabilityReason};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CStr,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};

struct Interfaces(*mut libc::ifaddrs);
impl Drop for Interfaces {
    fn drop(&mut self) {
        // SAFETY: this list was allocated by getifaddrs and is freed once.
        unsafe { libc::freeifaddrs(self.0) };
    }
}

/// Listeners may bind loopback and tailnet ranges only: never LAN, public or wildcard.
pub fn permitted_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1])
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

fn is_tailnet_interface(name: &str, address: IpAddr) -> bool {
    // macOS names the tailnet tunnel `utunN`; Linux's tailscaled names it `tailscale0`.
    let tunnel = name == "tailscale0"
        || name
            .strip_prefix("utun")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()));
    tunnel && !address.is_loopback() && permitted_address(address)
}

pub fn tailnet_addresses() -> io::Result<Vec<IpAddr>> {
    let mut first = std::ptr::null_mut();
    // SAFETY: first points to writable storage for the allocated list pointer.
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let list = Interfaces(first);
    let mut current = list.0;
    let mut addresses = BTreeSet::new();
    while !current.is_null() {
        // SAFETY: current belongs to the live getifaddrs list until list drops.
        let interface = unsafe { &*current };
        current = interface.ifa_next;
        if interface.ifa_addr.is_null()
            || interface.ifa_name.is_null()
            || interface.ifa_flags & libc::IFF_UP as u32 == 0
        {
            continue;
        }
        // SAFETY: getifaddrs supplies a nul-terminated interface name.
        let name = unsafe { CStr::from_ptr(interface.ifa_name) }.to_string_lossy();
        // SAFETY: a non-null address contains sockaddr and its declared family.
        let family = unsafe { (*interface.ifa_addr).sa_family } as i32;
        let address = match family {
            libc::AF_INET => {
                // SAFETY: AF_INET addresses are sockaddr_in values.
                let raw = unsafe { &*interface.ifa_addr.cast::<libc::sockaddr_in>() };
                IpAddr::V4(Ipv4Addr::from(raw.sin_addr.s_addr.to_ne_bytes()))
            }
            libc::AF_INET6 => {
                // SAFETY: AF_INET6 addresses are sockaddr_in6 values.
                let raw = unsafe { &*interface.ifa_addr.cast::<libc::sockaddr_in6>() };
                IpAddr::V6(Ipv6Addr::from(raw.sin6_addr.s6_addr))
            }
            _ => continue,
        };
        if is_tailnet_interface(&name, address) {
            addresses.insert(address);
        }
    }
    Ok(addresses.into_iter().collect())
}

/// How often the engine looks for tailnet addresses (ADR 0078).
pub const TAILNET_LOOKUP_PERIOD: Duration = Duration::from_secs(2);

pub fn reachable() -> EngineReachability {
    EngineReachability {
        remote: true,
        reason: None,
    }
}

pub fn unreachable(reason: EngineReachabilityReason) -> EngineReachability {
    EngineReachability {
        remote: false,
        reason: Some(reason),
    }
}

/// Nothing but loopback listens: the state before any listener starts.
pub fn loopback_only() -> EngineReachability {
    unreachable(EngineReachabilityReason::LoopbackOnly)
}

/// Reachability of the fixed listener set given with `--listen`.
pub fn fixed_reachability(listen: &[SocketAddr]) -> EngineReachability {
    if listen.iter().any(|address| !address.ip().is_loopback()) {
        reachable()
    } else {
        loopback_only()
    }
}

/// What clients see after a lookup changed it.
#[derive(Debug, Clone, PartialEq)]
pub struct TailnetUpdate {
    pub reachability: EngineReachability,
    /// The address devices should dial; `None` while only loopback listens.
    pub pair_address: Option<SocketAddr>,
}

pub type Lookup = Box<dyn FnMut() -> io::Result<Vec<IpAddr>> + Send>;
pub type Serve = Box<dyn FnMut(TcpListener) -> JoinHandle<io::Result<()>> + Send>;
pub type Log = Box<dyn FnMut(Value) + Send>;

/// Tailnet listeners that follow the interface addresses without an engine restart:
/// a listener starts when its address appears and stops when the address goes away.
pub struct TailnetListeners {
    port: u16,
    lookup: Lookup,
    serve: Serve,
    log: Log,
    active: BTreeMap<IpAddr, (SocketAddr, JoinHandle<io::Result<()>>)>,
    refused: BTreeSet<IpAddr>,
    lookup_failing: bool,
    reported: Option<TailnetUpdate>,
}

impl TailnetListeners {
    pub fn new(port: u16, lookup: Lookup, serve: Serve, log: Log) -> Self {
        Self {
            port,
            lookup,
            serve,
            log,
            active: BTreeMap::new(),
            refused: BTreeSet::new(),
            lookup_failing: false,
            reported: None,
        }
    }

    pub fn system(port: u16, serve: Serve) -> Self {
        Self::new(
            port,
            Box::new(tailnet_addresses),
            serve,
            Box::new(|event| eprintln!("{event}")),
        )
    }

    /// Looks the tailnet up once and starts or stops listeners to match. Returns an
    /// update only when reachability or the pairing address changed.
    pub async fn refresh(&mut self) -> Option<TailnetUpdate> {
        self.reap_stopped().await;
        let lookup_failed = match (self.lookup)() {
            Ok(addresses) => {
                self.lookup_failing = false;
                self.follow(addresses.into_iter().collect()).await;
                false
            }
            Err(error) => {
                // A failed enumeration is not evidence that an interface went away,
                // so running listeners stay up; the failure is logged once per streak.
                if !std::mem::replace(&mut self.lookup_failing, true) {
                    (self.log)(json!({
                        "event":"engine.tailnet_lookup_failed",
                        "error":io_kind(&error),
                        "retrySec":TAILNET_LOOKUP_PERIOD.as_secs(),
                    }));
                }
                true
            }
        };
        let reachability = if !self.active.is_empty() {
            reachable()
        } else if lookup_failed {
            unreachable(EngineReachabilityReason::TailnetLookupFailed)
        } else if !self.refused.is_empty() {
            unreachable(EngineReachabilityReason::TailnetListenFailed)
        } else {
            unreachable(EngineReachabilityReason::TailnetAddressNotFound)
        };
        let update = TailnetUpdate {
            pair_address: self
                .active
                .values()
                .next_back()
                .map(|(address, _)| *address),
            reachability,
        };
        if self.reported.as_ref() == Some(&update) {
            return None;
        }
        if self.reported.as_ref().map(|old| &old.reachability) != Some(&update.reachability) {
            (self.log)(reachability_event(&update.reachability, self.active.len()));
        }
        self.reported = Some(update.clone());
        Some(update)
    }

    /// Refreshes forever, handing each change to `apply`.
    pub async fn run(mut self, mut apply: impl FnMut(TailnetUpdate) -> Result<()>) -> Result<()> {
        let mut interval = tokio::time::interval(TAILNET_LOOKUP_PERIOD);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Some(update) = self.refresh().await {
                apply(update)?;
            }
        }
    }

    async fn follow(&mut self, wanted: BTreeSet<IpAddr>) {
        let wanted: BTreeSet<_> = wanted
            .into_iter()
            .filter(|ip| permitted_address(*ip))
            .collect();
        self.active.retain(|ip, (_, task)| {
            let keep = wanted.contains(ip);
            if !keep {
                task.abort();
            }
            keep
        });
        self.refused.retain(|ip| wanted.contains(ip));
        for ip in wanted {
            if self.active.contains_key(&ip) {
                continue;
            }
            let bound = match TcpListener::bind(SocketAddr::new(ip, self.port)).await {
                Ok(listener) => listener.local_addr().map(|address| (address, listener)),
                Err(error) => Err(error),
            };
            match bound {
                Ok((address, listener)) => {
                    self.refused.remove(&ip);
                    self.active.insert(ip, (address, (self.serve)(listener)));
                }
                // Retried on every lookup; logged once until the address binds or goes.
                Err(error) => {
                    if self.refused.insert(ip) {
                        (self.log)(json!({
                            "event":"engine.tailnet_listen_failed",
                            "family":family(ip),
                            "error":io_kind(&error),
                        }));
                    }
                }
            }
        }
    }

    async fn reap_stopped(&mut self) {
        let stopped: Vec<IpAddr> = self
            .active
            .iter()
            .filter(|(_, (_, task))| task.is_finished())
            .map(|(ip, _)| *ip)
            .collect();
        for ip in stopped {
            let Some((_, task)) = self.active.remove(&ip) else {
                continue;
            };
            let error = match task.await {
                Ok(Ok(())) => "Stopped".to_string(),
                Ok(Err(error)) => io_kind(&error),
                Err(join) if join.is_panic() => "Panicked".to_string(),
                Err(_) => "Cancelled".to_string(),
            };
            // The next lookup binds the address again if it is still present.
            (self.log)(json!({
                "event":"engine.tailnet_listener_stopped",
                "family":family(ip),
                "error":error,
            }));
        }
    }
}

/// Applies the first lookup before any client connects, so the first `engine.status` and
/// pairing link are already right, and returns the task that keeps following the tailnet.
pub(crate) async fn follow_tailnet(
    mut tailnet: TailnetListeners,
    core: Arc<Mutex<Core>>,
) -> Result<impl Future<Output = Result<()>> + Send + use<>> {
    let loopback = lock(&core)?.pair_address.clone();
    let apply = move |update| lock(&core)?.apply_tailnet(update, &loopback);
    if let Some(update) = tailnet.refresh().await {
        apply(update)?;
    }
    Ok(tailnet.run(apply))
}

fn lock(core: &Mutex<Core>) -> Result<MutexGuard<'_, Core>> {
    core.lock().map_err(|_| Error::EngineStatePoisoned)
}

impl Drop for TailnetListeners {
    fn drop(&mut self) {
        for (_, task) in self.active.values() {
            task.abort();
        }
    }
}

fn reachability_event(reachability: &EngineReachability, listeners: usize) -> Value {
    match &reachability.reason {
        None => json!({"event":"engine.tailnet_reachable","listeners":listeners}),
        Some(reason) => json!({
            "event":"engine.tailnet_unreachable",
            "reason":reason,
            "detail":"Not reachable from your devices; local clients keep loopback",
            "retrySec":TAILNET_LOOKUP_PERIOD.as_secs(),
        }),
    }
}

fn family(ip: IpAddr) -> &'static str {
    if ip.is_ipv4() { "ipv4" } else { "ipv6" }
}

/// The error kind only: tailnet logs never carry addresses or interface names.
fn io_kind(error: &io::Error) -> String {
    format!("{:?}", error.kind())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    type Script = Arc<Mutex<VecDeque<io::Result<Vec<IpAddr>>>>>;

    fn scripted(script: &Script) -> Lookup {
        let script = Arc::clone(script);
        Box::new(move || {
            script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(vec![]))
        })
    }

    fn accepting() -> Serve {
        Box::new(|listener: TcpListener| {
            tokio::spawn(async move {
                loop {
                    drop(listener.accept().await?);
                }
            })
        })
    }

    fn recorder() -> (Log, Arc<Mutex<Vec<Value>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        (
            Box::new(move |event| sink.lock().unwrap().push(event)),
            events,
        )
    }

    fn names(events: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
        events
            .lock()
            .unwrap()
            .iter()
            .map(|event| event["event"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn missing_tailnet_is_reported_and_a_late_address_starts_listening_without_restart() {
        let late: IpAddr = "::1".parse().unwrap();
        let script: Script = Arc::new(Mutex::new(VecDeque::from([
            Err(io::Error::from(io::ErrorKind::Interrupted)),
            Err(io::Error::from(io::ErrorKind::Interrupted)),
            Ok(vec![]),
            Ok(vec![]),
            Ok(vec![late]),
            Ok(vec![late]),
            Ok(vec![]),
        ])));
        let (log, events) = recorder();
        let mut tailnet = TailnetListeners::new(0, scripted(&script), accepting(), log);

        let failed = tailnet.refresh().await.unwrap();
        assert_eq!(
            failed.reachability,
            unreachable(EngineReachabilityReason::TailnetLookupFailed)
        );
        assert_eq!(failed.pair_address, None);
        assert_eq!(
            tailnet.refresh().await,
            None,
            "a repeated state is not re-announced"
        );

        let missing = tailnet.refresh().await.unwrap();
        assert_eq!(
            missing.reachability,
            unreachable(EngineReachabilityReason::TailnetAddressNotFound)
        );
        assert_eq!(tailnet.refresh().await, None);

        let found = tailnet.refresh().await.unwrap();
        assert_eq!(found.reachability, reachable());
        let address = found.pair_address.unwrap();
        assert_eq!(address.ip(), late);
        tokio::net::TcpStream::connect(address).await.unwrap();
        assert_eq!(tailnet.refresh().await, None);

        let gone = tailnet.refresh().await.unwrap();
        assert_eq!(
            gone.reachability,
            unreachable(EngineReachabilityReason::TailnetAddressNotFound)
        );
        assert_eq!(
            names(&events),
            [
                "engine.tailnet_lookup_failed",
                "engine.tailnet_unreachable",
                "engine.tailnet_unreachable",
                "engine.tailnet_reachable",
                "engine.tailnet_unreachable",
            ]
        );
        let logged = events.lock().unwrap();
        assert_eq!(logged[1]["reason"], "tailnet_lookup_failed");
        assert_eq!(logged[2]["reason"], "tailnet_address_not_found");
        assert_eq!(logged[3]["listeners"], 1);
        let text = serde_json::to_string(&*logged).unwrap();
        assert!(!text.contains("::1"), "logs never carry addresses: {text}");
    }

    #[tokio::test]
    async fn lookup_errors_keep_running_listeners_and_bind_failures_are_reported_once() {
        let local: IpAddr = "::1".parse().unwrap();
        let public: IpAddr = "8.8.8.8".parse().unwrap();
        let script: Script = Arc::new(Mutex::new(VecDeque::from([
            Ok(vec![local, public]),
            Err(io::Error::from(io::ErrorKind::Interrupted)),
        ])));
        let (log, events) = recorder();
        let mut tailnet = TailnetListeners::new(0, scripted(&script), accepting(), log);
        let up = tailnet.refresh().await.unwrap();
        assert_eq!(up.reachability, reachable());
        assert_eq!(
            up.pair_address.unwrap().ip(),
            local,
            "public addresses are never bound"
        );
        assert_eq!(
            tailnet.refresh().await,
            None,
            "a lookup error keeps the listener"
        );
        tokio::net::TcpStream::connect(up.pair_address.unwrap())
            .await
            .unwrap();
        assert_eq!(
            names(&events),
            ["engine.tailnet_reachable", "engine.tailnet_lookup_failed"]
        );

        let occupied = std::net::TcpListener::bind("[::1]:0").unwrap();
        let port = occupied.local_addr().unwrap().port();
        let script: Script = Arc::new(Mutex::new(VecDeque::from([
            Ok(vec![local]),
            Ok(vec![local]),
        ])));
        let (log, events) = recorder();
        let mut tailnet = TailnetListeners::new(port, scripted(&script), accepting(), log);
        let refused = tailnet.refresh().await.unwrap();
        assert_eq!(
            refused.reachability,
            unreachable(EngineReachabilityReason::TailnetListenFailed)
        );
        assert_eq!(tailnet.refresh().await, None);
        assert_eq!(
            names(&events),
            ["engine.tailnet_listen_failed", "engine.tailnet_unreachable"]
        );
        assert_eq!(events.lock().unwrap()[0]["error"], "AddrInUse");
    }

    #[test]
    fn fixed_listeners_report_loopback_only() {
        let loopback: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let tailnet: SocketAddr = "100.80.1.2:1".parse().unwrap();
        assert_eq!(
            fixed_reachability(&[loopback]),
            unreachable(EngineReachabilityReason::LoopbackOnly)
        );
        assert_eq!(fixed_reachability(&[loopback, tailnet]), reachable());
    }

    #[test]
    fn listener_refuses_public_lan_and_wildcard_addresses() {
        for ip in ["0.0.0.0", "::", "192.168.1.10", "8.8.8.8", "100.128.0.1"] {
            assert!(!permitted_address(ip.parse().unwrap()));
        }
        for ip in [
            "127.0.0.1",
            "::1",
            "100.64.0.1",
            "100.127.255.254",
            "fd7a:115c:a1e0::1",
        ] {
            assert!(permitted_address(ip.parse().unwrap()));
        }
    }

    #[test]
    fn physical_interfaces_and_unrelated_tunnels_are_excluded() {
        assert!(!is_tailnet_interface("en0", "100.80.1.2".parse().unwrap()));
        assert!(!is_tailnet_interface(
            "utun0",
            "192.168.1.2".parse().unwrap()
        ));
        assert!(!is_tailnet_interface("utun0", "127.0.0.1".parse().unwrap()));
        assert!(!is_tailnet_interface(
            "utun-name",
            "100.80.1.2".parse().unwrap()
        ));
        assert!(is_tailnet_interface("utun7", "100.80.1.2".parse().unwrap()));
        assert!(is_tailnet_interface(
            "tailscale0",
            "100.80.1.2".parse().unwrap()
        ));
        assert!(!is_tailnet_interface(
            "tailscale1",
            "100.80.1.2".parse().unwrap()
        ));
        assert!(is_tailnet_interface(
            "utun7",
            "fd7a:115c:a1e0::2".parse().unwrap()
        ));
    }
}
