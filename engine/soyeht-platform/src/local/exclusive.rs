//! A loopback listening socket with `SO_EXCLUSIVEADDRUSE`.
//!
//! Without it another process of the same user can bind the same port with `SO_REUSEADDR` and
//! be handed connections meant for the listener. The standard library cannot set the option
//! before the bind, so the socket is made here and then given to `TcpListener`.

use std::{
    io,
    net::TcpListener,
    os::windows::io::{FromRawSocket, OwnedSocket},
    ptr,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, IN_ADDR, IN_ADDR_0, INVALID_SOCKET, IPPROTO_TCP, SO_EXCLUSIVEADDRUSE, SOCK_STREAM,
    SOCKADDR, SOCKADDR_IN, SOL_SOCKET, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSADATA,
    WSASocketW, WSAStartup, bind, listen, setsockopt,
};

const BACKLOG: i32 = 128;

fn check(status: i32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn bind_loopback() -> io::Result<TcpListener> {
    // SAFETY: WSADATA is plain data that the call fills; Winsock keeps a count of startups, so
    // this one is harmless next to the standard library's own.
    let mut data: WSADATA = unsafe { std::mem::zeroed() };
    // SAFETY: `data` is a valid out pointer for the call.
    let started = unsafe { WSAStartup(0x0202, &raw mut data) };
    if started != 0 {
        return Err(io::Error::from_raw_os_error(started));
    }
    // SAFETY: plain arguments; the flags are the ones the standard library creates sockets with.
    let raw = unsafe {
        WSASocketW(
            i32::from(AF_INET),
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
    let address = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_port: 0,
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: u32::from_ne_bytes([127, 0, 0, 1]),
            },
        },
        sin_zero: [0; 8],
    };
    // SAFETY: `address` is a valid IPv4 address structure of the size passed.
    check(unsafe {
        bind(
            raw,
            ptr::from_ref(&address).cast::<SOCKADDR>(),
            size_of::<SOCKADDR_IN>() as i32,
        )
    })?;
    // SAFETY: the socket is open and bound.
    check(unsafe { listen(raw, BACKLOG) })?;
    Ok(TcpListener::from(socket))
}
