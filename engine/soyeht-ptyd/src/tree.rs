//! The descendants of a process. A process group is not a process tree: an
//! adapter's child that starts its own session (a shell tool, a browser) is
//! outside the group and survives a kill of the group. Closing an instance
//! must end that child too.

use std::collections::{BTreeMap, BTreeSet};

/// Every process that descends from `root` by parent links, excluding `root`.
pub fn descendants(root: u32) -> Vec<u32> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (pid, parent) in parent_links() {
        children.entry(parent).or_default().push(pid);
    }
    let mut found = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(parent) = pending.pop() {
        for child in children.get(&parent).into_iter().flatten() {
            if *child != root && found.insert(*child) {
                pending.push(*child);
            }
        }
    }
    found.into_iter().collect()
}

#[cfg(target_os = "macos")]
fn parent_links() -> Vec<(u32, u32)> {
    // SAFETY: a null buffer with size 0 only asks for the number of pids.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let Ok(capacity) = usize::try_from(count) else {
        return Vec::new();
    };
    // Processes may start between the two calls; leave room for them.
    let mut pids = vec![0 as libc::pid_t; capacity + capacity / 4 + 16];
    let size = i32::try_from(pids.len() * std::mem::size_of::<libc::pid_t>()).unwrap_or(i32::MAX);
    // SAFETY: the buffer is `size` bytes of pid_t that the call fills.
    let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
    let Ok(filled) = usize::try_from(filled) else {
        return Vec::new();
    };
    pids.truncate(filled);
    pids.into_iter()
        .filter(|pid| *pid > 0)
        .filter_map(|pid| {
            // SAFETY: proc_bsdinfo is plain data that the call fills.
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            // SAFETY: the buffer is a proc_bsdinfo of `size` bytes.
            let read = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    std::ptr::from_mut(&mut info).cast(),
                    size,
                )
            };
            (read == size).then_some((pid as u32, info.pbi_ppid))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn parent_links() -> Vec<(u32, u32)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            // Processes exit during the scan; any entry that cannot be read is skipped.
            let pid: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // The command name is parenthesized and may contain example.
            let after_name = stat.rsplit_once(')')?.1;
            let parent = after_name.split_whitespace().nth(1)?.parse().ok()?;
            Some((pid, parent))
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn parent_links() -> Vec<(u32, u32)> {
    Vec::new()
}
