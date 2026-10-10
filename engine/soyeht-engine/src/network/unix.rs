use std::{
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

/// The name and address of every address on an interface that is up.
pub(super) fn interface_addresses() -> io::Result<Vec<(String, IpAddr)>> {
    let mut first = std::ptr::null_mut();
    // SAFETY: first points to writable storage for the allocated list pointer.
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let list = Interfaces(first);
    let mut current = list.0;
    let mut addresses = Vec::new();
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
        addresses.push((name.into_owned(), address));
    }
    Ok(addresses)
}
