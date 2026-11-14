//! A listener that takes only the packets that arrive through one network interface (Linux).

use std::{
    io,
    mem::size_of,
    net::{IpAddr, SocketAddr, TcpListener},
    os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
};

/// The queue the standard library gives its listeners.
const BACKLOG: libc::c_int = 128;

/// Listens on `address` with `SO_BINDTODEVICE` set to `device`: only packets that come in through
/// `device` are delivered to this listener, so a host on another network that sends packets for
/// the address through another interface is refused by the kernel.
///
/// It does not keep this computer's own programs out: a program that connects to an address of
/// `device` is delivered as arriving through `device`, from that same address (checked on Linux 7.0),
/// so whoever accepts must read the peer address (ADR 0164).
///
/// Since Linux 5.7 no privilege is needed to set it on a socket that is not bound to a device yet.
pub(super) fn listen(address: SocketAddr, device: &str) -> io::Result<TcpListener> {
    let family = if address.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    // SAFETY: socket has no pointer arguments.
    let raw = unsafe { libc::socket(family, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw is a new descriptor that nothing else owns.
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };
    set_option(&socket, libc::SOL_SOCKET, libc::SO_REUSEADDR, &1_i32)?;
    let name = device.as_bytes();
    // SAFETY: the pointer and length name the bytes of `device`, which setsockopt only reads.
    let set = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            name.as_ptr().cast(),
            libc::socklen_t::try_from(name.len())
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
        )
    };
    if set != 0 {
        return Err(io::Error::last_os_error());
    }
    bind(&socket, address)?;
    // SAFETY: listen takes only the descriptor and a number.
    if unsafe { libc::listen(socket.as_raw_fd(), BACKLOG) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(TcpListener::from(socket))
}

fn set_option<T>(
    socket: &OwnedFd,
    level: libc::c_int,
    option: libc::c_int,
    value: &T,
) -> io::Result<()> {
    // SAFETY: the pointer and length describe `value`, which setsockopt only reads.
    let set = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            level,
            option,
            std::ptr::from_ref(value).cast(),
            size_of::<T>() as libc::socklen_t,
        )
    };
    if set == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn bind(socket: &OwnedFd, address: SocketAddr) -> io::Result<()> {
    let port = address.port().to_be();
    match address.ip() {
        IpAddr::V4(ip) => {
            // SAFETY: an all-zero sockaddr_in is a valid value of the type.
            let mut raw: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            raw.sin_family = libc::AF_INET as libc::sa_family_t;
            raw.sin_port = port;
            raw.sin_addr = libc::in_addr {
                s_addr: u32::from_ne_bytes(ip.octets()),
            };
            bind_raw(socket, &raw)
        }
        IpAddr::V6(ip) => {
            // SAFETY: an all-zero sockaddr_in6 is a valid value of the type.
            let mut raw: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            raw.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            raw.sin6_port = port;
            raw.sin6_addr = libc::in6_addr {
                s6_addr: ip.octets(),
            };
            bind_raw(socket, &raw)
        }
    }
}

/// `bind` to the socket address `raw`, one of the two `sockaddr_in` forms.
fn bind_raw<T>(socket: &OwnedFd, raw: &T) -> io::Result<()> {
    // SAFETY: the pointer and length describe `raw`, which bind only reads.
    let bound = unsafe {
        libc::bind(
            socket.as_raw_fd(),
            std::ptr::from_ref(raw).cast(),
            size_of::<T>() as libc::socklen_t,
        )
    };
    if bound == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
