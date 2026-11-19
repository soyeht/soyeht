//! A pane's saved launch follows the catalogue.

use super::{adapter_install, registry};
use soyeht_protocol::HarnessLaunch;
use std::path::Path;

/// The launch a restored pane of a built-in harness starts with when the catalogue has moved its
/// locked adapter to another version since the pane saved its own: the saved launch with the
/// catalogue's arguments and version. Without it the pane would name a package the engine no
/// longer installs from a lock. The catalogue's entry is read as written, so a program missing
/// from this machine's path (`npx`) does not skip it. `None` when nothing differs, for a custom
/// entry, for an adapter that is not locked (its acknowledgement is per version, and stays the
/// person's), and when the catalogue has no such entry.
pub(crate) fn refreshed_launch(directory: &Path, saved: &HarnessLaunch) -> Option<HarnessLaunch> {
    if !saved.id.starts_with(registry::BUILTIN_PREFIX) {
        return None;
    }
    registry::catalogue_launch(directory, &saved.id)
        .map_or(None, |current| followed(saved, &current))
}

fn followed(saved: &HarnessLaunch, current: &HarnessLaunch) -> Option<HarnessLaunch> {
    let locked = adapter_install::runner_package(current)?.1;
    (saved.id.starts_with(registry::BUILTIN_PREFIX)
        && locked
        && (current.args != saved.args || current.version != saved.version))
        .then(|| HarnessLaunch {
            args: current.args.clone(),
            version: current.version.clone(),
            ..saved.clone()
        })
}

#[cfg(test)]
mod tests;
