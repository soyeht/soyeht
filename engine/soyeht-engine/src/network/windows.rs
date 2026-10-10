use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};
use windows_sys::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS},
    NetworkManagement::{
        IpHelper::{
            GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
            GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6},
};

/// The friendly name and address of every address on an adapter that is up.
pub(super) fn interface_addresses() -> io::Result<Vec<(String, IpAddr)>> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    // 8-byte aligned storage for the linked structures the call writes.
    let mut buffer = vec![0_u64; 4 * 1024];
    loop {
        let mut bytes = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
        // SAFETY: `buffer` holds `bytes` writable bytes, aligned for the structures.
        let status = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>(),
                &mut bytes,
            )
        };
        match status {
            ERROR_SUCCESS => break,
            ERROR_BUFFER_OVERFLOW => buffer = vec![0_u64; (bytes as usize).div_ceil(8) + 1],
            error => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
    let mut addresses = Vec::new();
    let mut adapter = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !adapter.is_null() {
        // SAFETY: the list was written into `buffer`, which outlives this loop.
        let current = unsafe { &*adapter };
        adapter = current.Next;
        if current.OperStatus != IfOperStatusUp || current.FriendlyName.is_null() {
            continue;
        }
        // SAFETY: the friendly name is a NUL-terminated wide string inside `buffer`.
        let name = unsafe {
            let mut length = 0;
            while *current.FriendlyName.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(current.FriendlyName, length))
        };
        let mut unicast = current.FirstUnicastAddress;
        while !unicast.is_null() {
            // SAFETY: unicast entries are part of the same list.
            let entry = unsafe { &*unicast };
            unicast = entry.Next;
            let socket = entry.Address.lpSockaddr;
            if socket.is_null() {
                continue;
            }
            // SAFETY: a non-null socket address begins with its family.
            let family = unsafe { (*socket).sa_family };
            let address = if family == AF_INET {
                // SAFETY: AF_INET addresses are SOCKADDR_IN values.
                let raw = unsafe { &*socket.cast::<SOCKADDR_IN>() };
                // SAFETY: reading the union as bytes is valid for any initialised address.
                IpAddr::V4(Ipv4Addr::from(
                    unsafe { raw.sin_addr.S_un.S_addr }.to_ne_bytes(),
                ))
            } else if family == AF_INET6 {
                // SAFETY: AF_INET6 addresses are SOCKADDR_IN6 values.
                let raw = unsafe { &*socket.cast::<SOCKADDR_IN6>() };
                // SAFETY: reading the union as bytes is valid for any initialised address.
                IpAddr::V6(Ipv6Addr::from(unsafe { raw.sin6_addr.u.Byte }))
            } else {
                continue;
            };
            addresses.push((name.clone(), address));
        }
    }
    Ok(addresses)
}
