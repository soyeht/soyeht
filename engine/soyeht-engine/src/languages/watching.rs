//! When a language server needs the tick (ADR 0147, step 1b).
//!
//! With the supervisor's watch heard, a server is read when its process is marked, not on every
//! run of the tick; the sweep marks it once a second whatever the notices said. Without the watch
//! it is read on every run, as it always was.

use crate::{core::Core, read_ahead, tick_due::Next};

/// The processes of the language servers that run, for the sweep.
pub(crate) fn instances(core: &Core) -> Vec<String> {
    core.languages
        .keys()
        .iter()
        .filter_map(|key| core.languages.get(key)?.instance_id.clone())
        .collect()
}

/// Whether the server `key` has something for the tick: a write to send again, a read that
/// ended, or, with the watch heard, output the supervisor announced.
pub(crate) fn has_work(core: &Core, key: &str) -> bool {
    let Some(server) = core.languages.get(key) else {
        return false;
    };
    let Some(instance_id) = server.instance_id.as_deref() else {
        return false;
    };
    !core.watch.is_active()
        || !server.pending_writes.is_empty()
        || core.read_ready(&read_ahead::language(key))
        || core.watch.is_marked(instance_id)
}

/// When the tick must poll the language servers: now, while one has work. Their output, and the
/// end of their process, are announced by the supervisor and raise the tick.
pub(crate) fn due(core: &Core, next: &mut Next) {
    if core.languages.keys().iter().any(|key| has_work(core, key)) {
        next.now();
    }
}
