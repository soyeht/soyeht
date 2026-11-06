//! Which launches are package-runner adapters of the registry, and where they start (ADR 0143).

use super::registry::BUILTIN_PREFIX;
use soyeht_protocol::HarnessLaunch;
use std::path::Path;

/// The program a registry adapter is launched through: the package runner it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageRunner {
    Npx,
    Uvx,
}

/// The package runner `command` is, by its file stem (`npx.cmd` and `/opt/bin/npx` are `npx`;
/// `npx-cli.js` is the program `npx` runs; `uv` runs the same packages as `uvx`).
pub(crate) fn package_runner(command: &str) -> Option<PackageRunner> {
    let stem = Path::new(command)
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase();
    match stem.as_str() {
        "npx" | "npx-cli" => Some(PackageRunner::Npx),
        "uvx" | "uv" => Some(PackageRunner::Uvx),
        _ => None,
    }
}

/// Built-in adapters that read their project configuration from the folder they start in, not
/// from the session's folder (ADR 0143). They start in the workspace, as before.
const READS_PROJECT_FROM_ITS_FOLDER: &[&str] = &["registry.fast-agent"];

/// The package runner a built-in registry adapter is launched through. A custom harness is not
/// one, whatever its command: it runs as its author wrote it.
pub(crate) fn builtin_package_runner(launch: &HarnessLaunch) -> Option<PackageRunner> {
    launch
        .id
        .starts_with(BUILTIN_PREFIX)
        .then(|| package_runner(&launch.command))
        .flatten()
}

/// Whether the process of `launch` starts in the engine's own folder and not in the workspace:
/// a built-in adapter launched through a package runner, except the ones that need the workspace
/// as their folder.
pub(crate) fn starts_in_engine_folder(launch: &HarnessLaunch) -> bool {
    builtin_package_runner(launch).is_some()
        && !READS_PROJECT_FROM_ITS_FOLDER.contains(&launch.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_runners_are_known_by_their_file_stem_and_only_builtin_entries_are_adapters() {
        for command in [
            "npx",
            "/opt/homebrew/bin/npx",
            "C:/Program Files/nodejs/npx.cmd",
            "/node/bin/npx-cli.js",
            "uvx",
            "/usr/local/bin/uvx",
            "uv.exe",
        ] {
            assert!(package_runner(command).is_some(), "{command}");
        }
        for command in [
            "codex",
            "/usr/local/bin/claude",
            "node",
            "/bin/sh",
            "npxe",
            "",
        ] {
            assert!(package_runner(command).is_none(), "{command}");
        }
        let launch = |id: &str, command: &str| HarnessLaunch {
            id: id.into(),
            name: "Agent".into(),
            command: command.into(),
            args: vec!["--yes".into(), "package@1.0.0".into()],
            version: "1".into(),
            environment: None,
        };
        assert_eq!(
            builtin_package_runner(&launch("registry.claude-acp", "/bin/npx")),
            Some(PackageRunner::Npx)
        );
        // A custom harness that runs `npx ./local-agent` is not an adapter of the registry.
        assert_eq!(builtin_package_runner(&launch("my-agent", "npx")), None);
        assert_eq!(
            builtin_package_runner(&launch("registry.codex", "codex")),
            None
        );
        // The engine's folder is for the adapters that take their folder from the session.
        assert!(starts_in_engine_folder(&launch(
            "registry.claude-acp",
            "/bin/npx"
        )));
        assert!(!starts_in_engine_folder(&launch(
            "registry.fast-agent",
            "/bin/uvx"
        )));
        assert!(!starts_in_engine_folder(&launch("my-agent", "npx")));
    }
}
