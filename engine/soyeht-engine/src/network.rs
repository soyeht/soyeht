use std::{
    collections::BTreeSet,
    ffi::CStr,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

struct Interfaces(*mut libc::ifaddrs);
impl Drop for Interfaces {
    fn drop(&mut self) {
        // SAFETY: this list was allocated by getifaddrs and is freed once.
        unsafe { libc::freeifaddrs(self.0) };
    }
}

fn is_tailnet_interface(name: &str, address: IpAddr) -> bool {
    let tunnel = name
        .strip_prefix("utun")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()));
    tunnel && !address.is_loopback() && crate::server::permitted_address(address)
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

pub async fn wait_for_address_change(initial: Vec<IpAddr>) {
    wait_for_change(
        initial,
        std::time::Duration::from_secs(2),
        tailnet_addresses,
    )
    .await;
}

async fn wait_for_change(
    initial: Vec<IpAddr>,
    period: std::time::Duration,
    mut read: impl FnMut() -> io::Result<Vec<IpAddr>>,
) {
    let initial: BTreeSet<_> = initial.into_iter().collect();
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        // A transient enumeration failure is not evidence that the network changed.
        if let Ok(current) = read()
            && current.into_iter().collect::<BTreeSet<_>>() != initial
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn late_network_and_removal_trigger_restart_but_order_and_errors_do_not() {
        let first: IpAddr = "100.64.0.1".parse().unwrap();
        let second: IpAddr = "fd7a:115c:a1e0::1".parse().unwrap();
        let mut reads = 0;
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wait_for_change(vec![], std::time::Duration::from_millis(1), || {
                reads += 1;
                match reads {
                    1 => Err(io::Error::from(io::ErrorKind::Interrupted)),
                    2 => Ok(vec![]),
                    _ => Ok(vec![first]),
                }
            }),
        )
        .await
        .unwrap();
        assert_eq!(reads, 3);
        let mut reads = 0;
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wait_for_change(
                vec![first, second],
                std::time::Duration::from_millis(1),
                || {
                    reads += 1;
                    if reads == 1 {
                        Ok(vec![second, first])
                    } else {
                        Ok(vec![])
                    }
                },
            ),
        )
        .await
        .unwrap();
        assert_eq!(reads, 2);
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
            "utun7",
            "fd7a:115c:a1e0::2".parse().unwrap()
        ));
    }
}
