//! A listener bound to a network interface takes only the packets that arrive through it (ADR 0164),
//! which the kernel does not extend to a program of this machine that connects to the interface's
//! own address. Checked against the real system, so it runs on Linux only, and the part that needs
//! an interface with an IPv4 address other than loopback is skipped where there is none.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::net::listen_on_device;
use std::{
    ffi::CStr,
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    time::Duration,
};

/// The name and IPv4 address of an interface that is up and is not loopback, when there is one.
fn outward_interface() -> Option<(String, Ipv4Addr)> {
    let mut first = std::ptr::null_mut();
    // SAFETY: first points to writable storage for the allocated list pointer.
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return None;
    }
    let mut found = None;
    let mut current = first;
    while !current.is_null() {
        // SAFETY: current belongs to the live list until freeifaddrs below.
        let interface = unsafe { &*current };
        current = interface.ifa_next;
        if interface.ifa_addr.is_null()
            || interface.ifa_flags & libc::IFF_UP as u32 == 0
            || interface.ifa_flags & libc::IFF_LOOPBACK as u32 != 0
            // SAFETY: a non-null address holds a sockaddr with its family.
            || unsafe { (*interface.ifa_addr).sa_family } as i32 != libc::AF_INET
        {
            continue;
        }
        // SAFETY: AF_INET addresses are sockaddr_in values.
        let raw = unsafe { &*interface.ifa_addr.cast::<libc::sockaddr_in>() };
        // SAFETY: getifaddrs supplies a nul-terminated interface name.
        let name = unsafe { CStr::from_ptr(interface.ifa_name) }
            .to_string_lossy()
            .into_owned();
        found = Some((name, Ipv4Addr::from(raw.sin_addr.s_addr.to_ne_bytes())));
        break;
    }
    // SAFETY: first came from getifaddrs and is freed once.
    unsafe { libc::freeifaddrs(first) };
    found
}

fn connect(address: SocketAddr) -> io::Result<TcpStream> {
    TcpStream::connect_timeout(&address, Duration::from_secs(3))
}

#[test]
fn a_listener_takes_only_what_arrives_through_its_interface() {
    let Some((name, ip)) = outward_interface() else {
        eprintln!("skipped: no interface with a non-loopback IPv4 address");
        return;
    };
    // A connection to loopback arrives through loopback, not through the bound interface.
    let bound = listen_on_device(SocketAddr::from(([127, 0, 0, 1], 0)), &name).unwrap();
    let refused = connect(bound.local_addr().unwrap()).unwrap_err();
    assert_eq!(
        refused.kind(),
        io::ErrorKind::ConnectionRefused,
        "{refused}"
    );
    // The same address with no binding takes it: the refusal is the binding's.
    let plain = soyeht_platform::net::listen(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    assert!(connect(plain.local_addr().unwrap()).is_ok());
    // A program of this machine that connects to the interface's own address is NOT refused: the
    // kernel delivers it as arriving through the interface that owns the address, and its source
    // is that very address. It is the engine's rule about the peer's address, and not the
    // binding, that refuses it (ADR 0164).
    let own = listen_on_device(SocketAddr::new(IpAddr::V4(ip), 0), &name).unwrap();
    let stream = connect(own.local_addr().unwrap()).unwrap();
    let (_, peer) = own.accept().unwrap();
    assert_eq!(peer.ip(), IpAddr::V4(ip));
    assert_eq!(stream.local_addr().unwrap().ip(), IpAddr::V4(ip));
}

#[test]
fn a_listener_bound_to_loopback_takes_loopback() {
    let bound = listen_on_device(SocketAddr::from(([127, 0, 0, 1], 0)), "lo").unwrap();
    assert!(connect(bound.local_addr().unwrap()).is_ok());
    assert!(bound.accept().is_ok());
}

#[test]
fn an_interface_that_does_not_exist_is_an_error_and_the_address_is_not_taken() {
    let error = listen_on_device(SocketAddr::from(([127, 0, 0, 1], 0)), "nope0xyz").unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::ENODEV), "{error}");
}

#[test]
fn a_listener_bound_to_an_interface_can_be_made_again_on_the_same_port_after_it_is_dropped() {
    // The tailnet follower stops a listener when its address goes away and binds again when it
    // returns: the port must be free again at once.
    let first = listen_on_device(SocketAddr::from(([127, 0, 0, 1], 0)), "lo").unwrap();
    let address = first.local_addr().unwrap();
    drop(first);
    let again = listen_on_device(address, "lo").unwrap();
    assert_eq!(again.local_addr().unwrap(), address);
    // Another listener on the same address and port is refused while the first lives.
    let clash = listen_on_device(address, "lo").unwrap_err();
    assert_eq!(clash.kind(), io::ErrorKind::AddrInUse);
}
