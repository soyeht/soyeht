//! A listening socket with `SO_EXCLUSIVEADDRUSE`.
//!
//! Without it another process, of any user, can bind the same address and port with
//! `SO_REUSEADDR` and be handed connections meant for the listener. The standard library cannot
//! set the option before the bind, so the socket is made here and then given to `TcpListener`.

use std::{
    io,
    net::{SocketAddr, TcpListener},
    os::windows::io::{FromRawSocket, OwnedSocket},
    ptr,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, IN_ADDR, IN_ADDR_0, IN6_ADDR, IN6_ADDR_0, INVALID_SOCKET, IPPROTO_TCP,
    SO_EXCLUSIVEADDRUSE, SOCK_STREAM, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_IN6_0,
    SOL_SOCKET, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSADATA, WSASocketW, WSAStartup,
    bind, listen, setsockopt,
};

const BACKLOG: i32 = 128;

fn check(status: i32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Binds `socket` to `address`.
fn bind_to(socket: usize, address: SocketAddr) -> io::Result<()> {
    match address {
        SocketAddr::V4(address) => {
            let name = SOCKADDR_IN {
                sin_family: AF_INET,
                sin_port: address.port().to_be(),
                sin_addr: IN_ADDR {
                    S_un: IN_ADDR_0 {
                        S_addr: u32::from_ne_bytes(address.ip().octets()),
                    },
                },
                sin_zero: [0; 8],
            };
            // SAFETY: `name` is a valid IPv4 address structure of the size passed.
            check(unsafe {
                bind(
                    socket,
                    ptr::from_ref(&name).cast::<SOCKADDR>(),
                    size_of::<SOCKADDR_IN>() as i32,
                )
            })
        }
        SocketAddr::V6(address) => {
            let name = SOCKADDR_IN6 {
                sin6_family: AF_INET6,
                sin6_port: address.port().to_be(),
                sin6_flowinfo: address.flowinfo(),
                sin6_addr: IN6_ADDR {
                    u: IN6_ADDR_0 {
                        Byte: address.ip().octets(),
                    },
                },
                Anonymous: SOCKADDR_IN6_0 {
                    sin6_scope_id: address.scope_id(),
                },
            };
            // SAFETY: `name` is a valid IPv6 address structure of the size passed.
            check(unsafe {
                bind(
                    socket,
                    ptr::from_ref(&name).cast::<SOCKADDR>(),
                    size_of::<SOCKADDR_IN6>() as i32,
                )
            })
        }
    }
}

/// A listening socket on `address` that no other socket can share, whatever it asks for.
pub(super) fn exclusive_listener(address: SocketAddr) -> io::Result<TcpListener> {
    // SAFETY: WSADATA is plain data that the call fills; Winsock keeps a count of startups, so
    // this one is harmless next to the standard library's own.
    let mut data: WSADATA = unsafe { std::mem::zeroed() };
    // SAFETY: `data` is a valid out pointer for the call.
    let started = unsafe { WSAStartup(0x0202, &raw mut data) };
    if started != 0 {
        return Err(io::Error::from_raw_os_error(started));
    }
    let family = if address.is_ipv4() { AF_INET } else { AF_INET6 };
    // SAFETY: plain arguments; the flags are the ones the standard library creates sockets with.
    let raw = unsafe {
        WSASocketW(
            i32::from(family),
            SOCK_STREAM,
            IPPROTO_TCP,
            ptr::null(),
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    if raw == INVALID_SOCKET {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call returned a new socket that nothing else owns.
    let socket = unsafe { OwnedSocket::from_raw_socket(raw as u64) };
    let exclusive: u32 = 1;
    // SAFETY: the socket is open and `exclusive` lives for the call; the option takes a DWORD.
    check(unsafe {
        setsockopt(
            raw,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            ptr::from_ref(&exclusive).cast(),
            size_of::<u32>() as i32,
        )
    })?;
    bind_to(raw, address)?;
    // SAFETY: the socket is open and bound.
    check(unsafe { listen(raw, BACKLOG) })?;
    Ok(TcpListener::from(socket))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Networking::WinSock::{SO_REUSEADDR, closesocket};

    /// Whether a socket that asks for `SO_REUSEADDR`, as a program of another user may, gets to
    /// bind `address`.
    fn reuse_takes(address: SocketAddr) -> bool {
        let family = if address.is_ipv4() { AF_INET } else { AF_INET6 };
        // SAFETY: plain arguments.
        let raw = unsafe {
            WSASocketW(
                i32::from(family),
                SOCK_STREAM,
                IPPROTO_TCP,
                ptr::null(),
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            )
        };
        assert_ne!(raw, INVALID_SOCKET);
        let reuse: u32 = 1;
        // SAFETY: the socket is open and `reuse` lives for the call; the option takes a DWORD.
        check(unsafe {
            setsockopt(
                raw,
                SOL_SOCKET,
                SO_REUSEADDR,
                ptr::from_ref(&reuse).cast(),
                size_of::<u32>() as i32,
            )
        })
        .unwrap();
        let taken = bind_to(raw, address).is_ok();
        // SAFETY: the socket is open and nothing else owns it.
        unsafe { closesocket(raw) };
        taken
    }

    #[test]
    fn a_listener_cannot_be_taken_over_by_a_socket_that_asks_for_reuse() {
        for address in ["127.0.0.1:0", "[::1]:0"] {
            let listener = exclusive_listener(address.parse().unwrap()).unwrap();
            assert!(!reuse_takes(listener.local_addr().unwrap()), "{address}");
        }
    }
}
