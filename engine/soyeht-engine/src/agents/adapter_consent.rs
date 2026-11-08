//! The person's acknowledgement of a package an adapter downloads unchecked (ADR 0143).
//!
//! A built-in adapter that a package runner fetches (`npx --yes <package>@<version>`, `uvx
//! <package>==<version>`) and that the engine does not install from a lockfile
//! (`adapter_install`) runs the package and every dependency the registry serves at that
//! moment. The catalogue tells the person so before they start it; this is the rule that makes
//! the notice more than text: such an agent starts only after its exact `name@version` was
//! acknowledged once.
//!
//! - **Only a person acknowledges.** The acknowledgement is a field of the requests that choose
//!   the agent (`pane.create`, `pane.harness.switch`), and the engine takes it from a person's
//!   connection only. Every other start (a task, a role, a restart, a recovery, a switch
//!   candidate) finds the stored acknowledgement or is refused.
//! - **One version at a time.** The stored key is the whole `name@version`, so a new version of
//!   the adapter asks again.
//! - **Every start checks.** The agent is checked where it is chosen, so a request that cannot
//!   start fails at once, and again where its process is sent (`Core::launch`), so a path that
//!   forgot the first check still cannot start it.

use super::{
    adapter_install,
    package_runner::{PackageRunner, package_runner},
    registry,
};
use crate::{
    core::Core,
    errors::{self, Result},
};
use soyeht_protocol::{HarnessLaunch, Role};
use soyeht_store::{adapter_acknowledgement::acknowledged, database::Store};
use std::{collections::BTreeSet, sync::OnceLock};

/// The package `launch` downloads and runs unchecked, as `name@version`; `None` for a harness
/// that runs no package (a program on the `PATH`), for a custom entry, and for an adapter the
/// engine installs from its lockfile.
pub(crate) fn unchecked_package(launch: &HarnessLaunch) -> Option<String> {
    adapter_install::runner_package(launch)
        .and_then(|(package, locked)| (!locked).then_some(package))
}

/// Every package a built-in adapter downloads unchecked.
fn unchecked_packages() -> &'static BTreeSet<String> {
    static PACKAGES: OnceLock<BTreeSet<String>> = OnceLock::new();
    PACKAGES.get_or_init(|| {
        // An invalid snapshot fails every catalogue read and every start with its own error.
        registry::builtin_entries()
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| unchecked_package(&entry.launch))
            .collect()
    })
}

/// The package a process started with `argv` would download unchecked: the one of a built-in
/// adapter's launch, where the process is sent. A custom entry's launch is its author's, and is
/// not in the snapshot.
fn unchecked_package_of(argv: &[String]) -> Option<&str> {
    let (runner, rest) = argv.split_first()?;
    let package = match package_runner(runner)? {
        PackageRunner::Npx => rest
            .first()
            .filter(|flag| *flag == "--yes")
            .and(rest.get(1)),
        PackageRunner::Uvx => rest.first(),
    }?;
    unchecked_packages()
        .contains(package.as_str())
        .then_some(package.as_str())
}

/// What the refusal says, and what a pane that stopped for it shows beside its dead state: the
/// package, so the person knows what to acknowledge.
fn refusal(package: &str) -> errors::Error {
    errors::adapter_not_acknowledged(format!(
        "Acknowledge {package} to start this agent: it downloads and runs the package and its \
         dependencies unchecked"
    ))
}

/// Refuses the start of `launch` unless its package was acknowledged, or `acknowledging` (the
/// package a person's request acknowledges now, see [`Core::acknowledgement_of`]) is it.
pub(crate) fn require(
    store: &Store,
    launch: &HarnessLaunch,
    acknowledging: Option<&str>,
) -> Result<()> {
    let Some(package) = unchecked_package(launch) else {
        return Ok(());
    };
    if acknowledging == Some(package.as_str())
        || acknowledged(store, &package).map_err(errors::storage)?
    {
        return Ok(());
    }
    Err(refusal(&package))
}

/// Refuses the start of a process with `argv` unless its package was acknowledged: the check at
/// the one place every process is sent from.
pub(crate) fn require_spawn(store: &Store, argv: &[String]) -> Result<()> {
    let Some(package) = unchecked_package_of(argv) else {
        return Ok(());
    };
    if acknowledged(store, package).map_err(errors::storage)? {
        return Ok(());
    }
    Err(refusal(package))
}

impl Core {
    /// The package a person's request acknowledges (`sent`, its `acknowledgedPackage`) for the agent
    /// of `launch`, to be stored with the change the request makes. `None` when the request carries none, or the agent downloads
    /// nothing unchecked. A request carrying a package other than the one the agent runs now (it
    /// moved to another version since the picker showed it) is refused, and so is one that does
    /// not come from a person.
    pub(crate) fn acknowledgement_of(
        &self,
        connection: &str,
        sent: Option<&str>,
        launch: &HarnessLaunch,
    ) -> Result<Option<String>> {
        let Some(sent) = sent else {
            return Ok(None);
        };
        let person = self
            .clients
            .get(connection)
            .and_then(|client| client.actor.as_ref())
            .is_some_and(|actor| matches!(actor.role, Role::HumanLocal | Role::HumanDevice));
        if !person {
            return Err(errors::forbidden_role(
                "Only a person may acknowledge a package",
            ));
        }
        let Some(package) = unchecked_package(launch) else {
            return Ok(None);
        };
        if sent != package {
            return Err(errors::invalid(
                "The acknowledged package is not the one this agent runs now",
            ));
        }
        Ok(Some(package))
    }
}

#[cfg(test)]
mod tests;
