use std::collections::{BTreeMap, BTreeSet};

/// What to send to a process group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupSignal {
    /// Ends every process of the group at once.
    Kill,
    /// Asks the group to stop, as closing its terminal does.
    Terminate,
}

/// Signals the process group that `leader` leads. A process that left the group with its own
/// session stays. Nothing happens for a pid that cannot name a group of ours (0 and 1 would
/// reach every process or the init process).
///
/// `leader` must be a child of this process that was started as a group leader, never a pid
/// loaded from a previous run's record. It may already have been reaped: the number stays
/// reserved as a group id while any member lives, so the signal still reaches the members that
/// outlived their leader. Once no member is left the number is free for reuse, so callers
/// signal right after they learn the leader ended, not later.
pub fn signal_group(leader: u32, signal: GroupSignal) {
    let signal = match signal {
        GroupSignal::Kill => libc::SIGKILL,
        GroupSignal::Terminate => libc::SIGTERM,
    };
    if let Ok(leader) = libc::pid_t::try_from(leader)
        && leader > 1
    {
        // SAFETY: signalling only sends a signal. The caller's child is not reaped, so its pid
        // still names the group it leads.
        unsafe {
            libc::kill(-leader, signal);
        }
    }
}

/// Sends the stop request to the process `pid`, which [`super::terminate`] has checked is neither
/// 0, 1 nor this process.
pub(super) fn terminate(pid: u32) -> std::io::Result<()> {
    let pid = libc::pid_t::try_from(pid)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: signalling only sends a signal; the caller got the pid from the lock the process
    // holds, which proves it is alive.
    if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    // The standard library has no kind for "no such process".
    if error.raw_os_error() == Some(libc::ESRCH) {
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    } else {
        Err(error)
    }
}

/// Ends `leader`, everything in the process group it leads, and every descendant that left
/// the group. This is the Unix form of `windows::Spawned::kill_tree`.
///
/// A process group is not a process tree: a child that starts its own session (a shell tool, a
/// browser) is outside the group and survives [`signal_group`]. The descendants are read
/// before the group is signalled, because once the leader dies its children are reparented
/// and no longer reachable from it. `leader` has the conditions of [`signal_group`].
pub fn kill_tree(leader: u32) {
    let escaped = descendants(leader);
    signal_group(leader, GroupSignal::Kill);
    for descendant in escaped {
        if let Ok(descendant) = libc::pid_t::try_from(descendant)
            && descendant > 1
        {
            // SAFETY: the pid was read from the process table an instant ago, as a
            // descendant of a child of this process.
            unsafe {
                libc::kill(descendant, libc::SIGKILL);
            }
        }
    }
}

/// Every process that descends from `root` by parent links, excluding `root`.
fn descendants(root: u32) -> Vec<u32> {
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
            // ignored: processes exit during the scan, so an entry that cannot be read is skipped
            let pid: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            // ignored: the process may have exited since the listing
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // The command name is parenthesized and may contain whitespace.
            let after_name = stat.rsplit_once(')')?.1;
            let parent = after_name.split_whitespace().nth(1)?.parse().ok()?;
            Some((pid, parent))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    fn alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    fn wait_gone(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if !alive(pid) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn kill_tree_ends_the_group_and_a_descendant_that_left_it() {
        let _forks = crate::spawning();
        // The shell leads a group, starts one sleeper in it and one in a session of its own.
        let mut leader = Command::new("sh")
            .args([
                "-c",
                "sleep 60 & echo $!; perl -MPOSIX -e 'setsid(); exec q(sleep), q(60)' & echo $!; wait",
            ])
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(leader.stdout.take().unwrap()).lines();
        let mut next_pid = || lines.next().unwrap().unwrap().parse::<u32>().unwrap();
        let (in_group, escaped) = (next_pid(), next_pid());
        // Give perl time to leave the group before the tree is read.
        thread::sleep(Duration::from_millis(500));
        assert!(alive(in_group) && alive(escaped));

        kill_tree(leader.id());

        leader.wait().unwrap();
        assert!(wait_gone(in_group), "the process in the group survived");
        assert!(
            wait_gone(escaped),
            "the process that left the group survived"
        );
    }

    #[test]
    fn the_descendants_of_a_process_are_found_by_parent_links() {
        let _forks = crate::spawning();
        let mut shell = Command::new("sh")
            .args(["-c", "sleep 60 & wait"])
            .spawn()
            .unwrap();
        thread::sleep(Duration::from_millis(300));
        let found = descendants(shell.id());
        for pid in &found {
            // SAFETY: SIGKILL to a process this test just found below its own child.
            unsafe { libc::kill(*pid as libc::pid_t, libc::SIGKILL) };
        }
        shell.kill().unwrap();
        shell.wait().unwrap();
        assert_eq!(found.len(), 1);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("the process tree is read on macOS and Linux only");
