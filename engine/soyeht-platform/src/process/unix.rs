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
