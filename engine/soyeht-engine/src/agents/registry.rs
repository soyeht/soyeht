use crate::errors::{self, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use soyeht_protocol::{HarnessEnvironmentVariable, HarnessLaunch, HarnessSource, limits};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

const BUILTIN_PREFIX: &str = "registry.";
const BUILTIN_SNAPSHOT: &str = include_str!("acp_registry_snapshot.json");
const BUILTIN_AGENT_COUNT: usize = 41;
const SOURCE_DIGEST: &str = "2e0ddd7be13f676b48158f266ba8d64a29da7c80e971b493664ed993dd99e295";
const SNAPSHOT_DIGEST: &str = "d41c4079f196bd29be2ae0f631f673950e8baa22da2e38e331a2d7e3a7a34240";

#[derive(Clone)]
pub(crate) struct RegistryEntry {
    pub launch: HarnessLaunch,
    pub source: HarnessSource,
    recipe_directories: Vec<PathBuf>,
    local_commands: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    source: String,
    source_version: String,
    source_sha256: String,
    agents: Vec<SnapshotEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotEntry {
    id: String,
    name: String,
    version: String,
    command: String,
    args: Vec<String>,
    #[serde(default)]
    environment: Vec<HarnessEnvironmentVariable>,
    #[serde(default)]
    user_directories: Vec<String>,
    #[serde(default)]
    local_commands: Vec<String>,
}

fn valid_environment(environment: &[HarnessEnvironmentVariable]) -> bool {
    let mut keys = BTreeSet::new();
    environment.len() <= 32
        && environment.iter().all(|item| {
            !item.key.is_empty()
                && item.key.len() <= 128
                && item
                    .key
                    .chars()
                    .all(|character| character == '_' || character.is_ascii_alphanumeric())
                && !item.value.contains('\0')
                && item.value.len() <= 4096
                && keys.insert(item.key.clone())
        })
}

fn valid_custom_environment(environment: &[HarnessEnvironmentVariable]) -> bool {
    valid_environment(environment)
        && environment.iter().all(|item| {
            !matches!(item.key.as_str(), "PATH" | "HOME" | "USER" | "LOGNAME")
                && !item.key.starts_with("SOYEHT_")
        })
}

fn direct_custom_command(command: &str) -> bool {
    let path = Path::new(command);
    let direct = path.is_absolute() || path.components().count() == 1;
    let blocked_wrapper = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                "env" | "sh" | "bash" | "dash" | "fish" | "ksh" | "zsh"
            )
        });
    direct && !blocked_wrapper
}

fn valid_launch(entry: &HarnessLaunch) -> bool {
    !entry.id.is_empty()
        && entry.id.chars().count() <= 128
        && !entry.name.is_empty()
        && entry.name.chars().count() <= 256
        && !entry.command.is_empty()
        && entry.command.len() <= 4096
        && !entry.command.contains('\0')
        && entry.version.len() <= 128
        && entry.args.len() <= 64
        && entry
            .args
            .iter()
            .all(|argument| argument.len() <= 4096 && !argument.contains('\0'))
        && entry.environment.as_deref().is_none_or(valid_environment)
}

fn valid_custom_launch(entry: &HarnessLaunch) -> bool {
    !entry.id.starts_with(BUILTIN_PREFIX)
        && valid_launch(entry)
        && direct_custom_command(&entry.command)
        && entry
            .environment
            .as_deref()
            .is_none_or(valid_custom_environment)
}

pub(super) fn pinned_package(value: &str) -> bool {
    !value.starts_with('-')
        && (value.rsplit_once('@').is_some_and(|(name, version)| {
            !name.is_empty() && !version.is_empty() && version != "latest"
        }) || value
            .rsplit_once("==")
            .is_some_and(|(name, version)| !name.is_empty() && !version.is_empty()))
}

fn valid_builtin_runner(entry: &HarnessLaunch) -> bool {
    match entry.command.as_str() {
        "npx" => {
            entry.args.first().is_some_and(|value| value == "--yes")
                && entry.args.get(1).is_some_and(|value| pinned_package(value))
        }
        "uvx" => entry
            .args
            .first()
            .is_some_and(|value| pinned_package(value)),
        _ => Path::new(&entry.command).components().count() == 1,
    }
}

fn builtin_entries() -> Result<Vec<RegistryEntry>> {
    if format!("{:x}", Sha256::digest(BUILTIN_SNAPSHOT.as_bytes())) != SNAPSHOT_DIGEST {
        return Err(errors::internal("Embedded ACP registry bytes changed"));
    }
    let snapshot: Snapshot = serde_json::from_str(BUILTIN_SNAPSHOT)
        .map_err(|_| errors::internal("Embedded ACP registry is invalid"))?;
    if snapshot.source != "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json"
        || snapshot.source_version != "1.0.0"
        || snapshot.source_sha256 != SOURCE_DIGEST
        || snapshot.agents.len() != BUILTIN_AGENT_COUNT
    {
        return Err(errors::internal("Embedded ACP registry identity changed"));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut seen = BTreeSet::new();
    snapshot
        .agents
        .into_iter()
        .map(|entry| {
            let directories_valid = entry.user_directories.len() <= 8
                && entry.user_directories.iter().all(|directory| {
                    !directory.is_empty()
                        && Path::new(directory)
                            .components()
                            .all(|component| matches!(component, Component::Normal(_)))
                });
            let local_commands_valid = entry.local_commands.len() <= 8
                && entry.local_commands.iter().all(|command| {
                    !command.is_empty()
                        && command.len() <= 128
                        && Path::new(command).components().count() == 1
                });
            let directories = entry
                .user_directories
                .iter()
                .filter_map(|directory| {
                    let relative = Path::new(directory);
                    home.as_ref()
                        .filter(|_| !relative.is_absolute())
                        .map(|home| home.join(relative))
                })
                .collect::<Vec<_>>();
            let launch = HarnessLaunch {
                id: entry.id,
                name: entry.name,
                command: entry.command,
                args: entry.args,
                version: entry.version,
                environment: (!entry.environment.is_empty()).then_some(entry.environment),
            };
            if !launch.id.starts_with(BUILTIN_PREFIX)
                || !seen.insert(launch.id.clone())
                || !valid_launch(&launch)
                || !valid_builtin_runner(&launch)
                || !directories_valid
                || !local_commands_valid
            {
                return Err(errors::internal("Embedded ACP registry entry is invalid"));
            }
            Ok(RegistryEntry {
                launch,
                source: HarnessSource::BuiltIn,
                recipe_directories: directories,
                local_commands: entry.local_commands,
            })
        })
        .collect()
}

/// This file belongs to the local profile. Remote pane creation can select an
/// entry by ID but cannot supply a command or change this launch configuration.
fn custom_bytes(directory: &Path) -> Result<Option<Vec<u8>>> {
    let path = directory.join("harnesses.json");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(errors::io(error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(errors::invalid("Harness registry must be a regular file"));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(errors::io)?;
    if !file.metadata().map_err(errors::io)?.is_file() {
        return Err(errors::invalid("Harness registry must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limits::JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(errors::io)?;
    if bytes.len() > limits::JSON_BYTES {
        return Err(errors::invalid("Harness registry exceeds 1 MiB"));
    }
    Ok(Some(bytes))
}

fn parse_custom_entries(bytes: Option<&[u8]>) -> Result<Vec<HarnessLaunch>> {
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let entries: Vec<HarnessLaunch> =
        serde_json::from_slice(bytes).map_err(|_| errors::invalid("Invalid harness registry"))?;
    let mut seen = BTreeSet::new();
    if entries.len() > 64
        || entries
            .iter()
            .any(|entry| !seen.insert(entry.id.clone()) || !valid_custom_launch(entry))
    {
        return Err(errors::invalid("Invalid or duplicate harness launch entry"));
    }
    Ok(entries)
}

pub(crate) fn custom_launches(directory: &Path) -> Result<Vec<HarnessLaunch>> {
    let bytes = custom_bytes(directory)?;
    parse_custom_entries(bytes.as_deref())
}

fn custom_entries(directory: &Path) -> Result<Vec<RegistryEntry>> {
    let entries = custom_launches(directory)?;
    Ok(entries
        .into_iter()
        .map(|launch| RegistryEntry {
            launch,
            source: HarnessSource::Custom,
            recipe_directories: Vec::new(),
            local_commands: Vec::new(),
        })
        .collect())
}

fn encoded_custom_entries(entries: &[HarnessLaunch]) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(entries)
        .map_err(|_| errors::internal("Harness registry serialization failed"))?;
    bytes.push(b'\n');
    if bytes.len() > limits::JSON_BYTES {
        return Err(errors::invalid("Harness registry exceeds 1 MiB"));
    }
    Ok(bytes)
}

fn registry_unchanged(directory: &Path, expected: Option<&[u8]>) -> Result<bool> {
    Ok(custom_bytes(directory)?.as_deref() == expected)
}

fn write_custom_entries(
    directory: &Path,
    expected: Option<&[u8]>,
    entries: &[HarnessLaunch],
) -> Result<()> {
    let bytes = encoded_custom_entries(entries)?;
    let temporary = directory.join(format!(".soyeht-harnesses-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(errors::io)?;
        file.write_all(&bytes).map_err(errors::io)?;
        file.sync_all().map_err(errors::io)?;
        if !registry_unchanged(directory, expected)? {
            return Err(errors::stale_version(
                "Harness registry changed while it was being edited",
            ));
        }
        fs::rename(&temporary, directory.join("harnesses.json")).map_err(errors::io)?;
        File::open(directory)
            .and_then(|parent| parent.sync_all())
            .map_err(errors::io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn create_custom(directory: &Path, launch: HarnessLaunch) -> Result<HarnessLaunch> {
    if !valid_custom_launch(&launch) {
        return Err(errors::invalid("Invalid custom harness launch entry"));
    }
    let prior = custom_bytes(directory)?;
    let mut entries = parse_custom_entries(prior.as_deref())?;
    if let Some(existing) = entries.iter().find(|entry| entry.id == launch.id) {
        return if existing == &launch {
            Ok(existing.clone())
        } else {
            Err(errors::idempotency_conflict(
                "Custom harness identifier is already in use",
            ))
        };
    }
    entries.push(launch.clone());
    write_custom_entries(directory, prior.as_deref(), &entries)?;
    Ok(launch)
}

pub(crate) fn update_custom(directory: &Path, launch: HarnessLaunch) -> Result<HarnessLaunch> {
    if !valid_custom_launch(&launch) {
        return Err(errors::invalid("Invalid custom harness launch entry"));
    }
    let prior = custom_bytes(directory)?;
    let mut entries = parse_custom_entries(prior.as_deref())?;
    let existing = entries
        .iter_mut()
        .find(|entry| entry.id == launch.id)
        .ok_or_else(|| errors::not_found("Custom harness not found"))?;
    if existing == &launch {
        return Ok(launch);
    }
    *existing = launch.clone();
    write_custom_entries(directory, prior.as_deref(), &entries)?;
    Ok(launch)
}

pub(crate) fn remove_custom(directory: &Path, id: &str) -> Result<()> {
    if id.is_empty() || id.chars().count() > 128 || id.starts_with(BUILTIN_PREFIX) {
        return Err(errors::invalid("Invalid custom harness identifier"));
    }
    let prior = custom_bytes(directory)?;
    let mut entries = parse_custom_entries(prior.as_deref())?;
    let count = entries.len();
    entries.retain(|entry| entry.id != id);
    if entries.len() == count {
        return Ok(());
    }
    write_custom_entries(directory, prior.as_deref(), &entries)
}

pub(crate) fn catalogue_entries(directory: &Path) -> Result<Vec<RegistryEntry>> {
    let mut entries = custom_entries(directory)?;
    entries.extend(builtin_entries()?);
    Ok(entries)
}

pub fn entries(directory: &Path) -> Result<Vec<HarnessLaunch>> {
    Ok(catalogue_entries(directory)?
        .into_iter()
        .map(|entry| entry.launch)
        .collect())
}

fn resolve_executable(
    command: &str,
    directories: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    let path = Path::new(command);
    let candidates = if path.is_absolute() {
        vec![path.to_owned()]
    } else if path.components().count() == 1 {
        directories
            .into_iter()
            .map(|directory| directory.join(command))
            .collect()
    } else {
        vec![]
    };
    candidates
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .find(|path| {
            path.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

pub fn executable(command: &str) -> Option<PathBuf> {
    resolve_executable(command, super::environment::candidate_directories(&[]))
}

pub(crate) fn resolve_entry(mut entry: RegistryEntry) -> Option<HarnessLaunch> {
    let path = resolve_executable(
        &entry.launch.command,
        super::environment::candidate_directories(&entry.recipe_directories),
    )?;
    entry.launch.command = path.to_str()?.into();
    Some(entry.launch)
}

// A registry package runner is locally owned only after a companion command is
// found or an ACP session previously opened. The runner alone is launchable,
// but its package remains unverified until the user explicitly starts it.
pub(crate) fn locally_installed(entry: &RegistryEntry, previous_success: bool) -> bool {
    if matches!(entry.source, HarnessSource::Custom)
        || !matches!(entry.launch.command.as_str(), "npx" | "uvx")
    {
        return resolve_entry(entry.clone()).is_some();
    }
    if previous_success {
        return resolve_entry(entry.clone()).is_some();
    }
    let directories = super::environment::candidate_directories(&entry.recipe_directories);
    entry
        .local_commands
        .iter()
        .any(|command| resolve_executable(command, directories.clone()).is_some())
}

fn digest_executable(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(errors::io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(errors::io)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn prepare_entry(entry: RegistryEntry) -> Result<(HarnessLaunch, String)> {
    let launch = resolve_entry(entry)
        .ok_or_else(|| errors::harness_unavailable("Harness executable is unavailable"))?;
    let digest = digest_executable(Path::new(&launch.command))?;
    Ok((launch, digest))
}

pub fn prepare(directory: &Path, id: &str) -> Result<(HarnessLaunch, String)> {
    let entry = catalogue_entries(directory)?
        .into_iter()
        .find(|entry| entry.launch.id == id)
        .ok_or_else(|| errors::harness_unavailable("Harness is not configured in this profile"))?;
    prepare_entry(entry)
}

pub(crate) fn prepare_local_terminal(
    directory: &Path,
    id: &str,
) -> Result<(HarnessLaunch, PathBuf)> {
    let entry = catalogue_entries(directory)?
        .into_iter()
        .find(|entry| entry.launch.id == id)
        .ok_or_else(|| errors::harness_unavailable("Harness is not configured in this profile"))?;
    let directories = super::environment::candidate_directories(&entry.recipe_directories);
    let command = entry
        .local_commands
        .iter()
        .find_map(|command| resolve_executable(command, directories.clone()))
        .ok_or_else(|| {
            errors::harness_unavailable("Harness has no local terminal command available")
        })?;
    Ok((entry.launch, command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use uuid::Uuid;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("soyeht-harness-path-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn executable(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn embedded_catalogue_is_pinned_complete_and_contains_required_agents() {
        let entries = builtin_entries().unwrap();
        assert_eq!(entries.len(), BUILTIN_AGENT_COUNT);
        let ids = entries
            .iter()
            .map(|entry| entry.launch.id.as_str())
            .collect::<BTreeSet<_>>();
        for id in [
            "registry.claude-acp",
            "registry.codex-acp",
            "registry.cursor",
            "registry.opencode",
            "registry.gemini",
        ] {
            assert!(ids.contains(id));
        }
        let codex = entries
            .iter()
            .find(|entry| entry.launch.id == "registry.codex-acp")
            .unwrap();
        assert_eq!(codex.launch.command, "npx");
        assert_eq!(codex.local_commands, ["codex"]);
        assert_eq!(
            codex.launch.args,
            ["--yes", "@agentclientprotocol/codex-acp@1.11.0"]
        );
        assert!(
            codex
                .launch
                .args
                .iter()
                .all(|argument| argument != "latest")
        );
    }

    #[test]
    fn package_runner_is_launchable_without_claiming_a_local_install() {
        let fixture = Fixture::new();
        let evidence = format!("owned-agent-{}", Uuid::new_v4());
        let runner = RegistryEntry {
            launch: HarnessLaunch {
                id: "registry.runner".into(),
                name: "Runner".into(),
                command: "npx".into(),
                args: vec!["--yes".into(), "runner@1.0.0".into()],
                version: "1.0.0".into(),
                environment: None,
            },
            source: HarnessSource::BuiltIn,
            recipe_directories: vec![fixture.0.clone()],
            local_commands: vec![evidence.clone()],
        };
        assert!(!locally_installed(&runner, false));
        fixture.executable(&evidence);
        assert!(locally_installed(&runner, false));

        let mut unowned = runner.clone();
        unowned.local_commands.clear();
        assert!(!locally_installed(&unowned, false));
    }

    #[test]
    fn custom_entries_are_first_and_cannot_claim_builtin_or_environment_identity() {
        let fixture = Fixture::new();
        fs::write(
            fixture.0.join("harnesses.json"),
            br#"[{"id":"mine","name":"Mine","command":"mine","args":[],"version":"1"}]"#,
        )
        .unwrap();
        let entries = catalogue_entries(&fixture.0).unwrap();
        assert_eq!(entries[0].launch.id, "mine");
        assert_eq!(entries[0].source, HarnessSource::Custom);
        fs::write(
            fixture.0.join("harnesses.json"),
            br#"[{"id":"registry.cursor","name":"Mine","command":"mine","args":[],"version":"1"}]"#,
        )
        .unwrap();
        assert!(catalogue_entries(&fixture.0).is_err());
        fs::write(
            fixture.0.join("harnesses.json"),
            br#"[{"id":"mine","name":"Mine","command":"mine","args":[],"version":"1","environment":[{"key":"HOME","value":"/tmp/empty"}]}]"#,
        )
        .unwrap();
        assert!(catalogue_entries(&fixture.0).is_err());
    }

    #[test]
    fn executable_resolution_uses_order_and_rejects_relative_paths() {
        let first = Fixture::new();
        let second = Fixture::new();
        let expected = first.executable("ordered-agent").canonicalize().unwrap();
        let _later = second.executable("ordered-agent");
        assert_eq!(
            resolve_executable("ordered-agent", [first.0.clone(), second.0.clone()]),
            Some(expected)
        );
        assert_eq!(
            resolve_executable("nested/agent", [first.0.clone(), second.0.clone()]),
            None
        );
    }

    #[test]
    fn custom_registry_is_atomic_bounded_and_rejects_identity_wrappers() {
        let fixture = Fixture::new();
        let command = fixture.executable("custom-agent");
        let launch = HarnessLaunch {
            id: "custom-agent".into(),
            name: "Custom agent".into(),
            command: command.to_string_lossy().into_owned(),
            args: vec!["acp".into()],
            version: "1".into(),
            environment: None,
        };
        assert_eq!(create_custom(&fixture.0, launch.clone()).unwrap(), launch);
        assert_eq!(
            custom_launches(&fixture.0).unwrap(),
            std::slice::from_ref(&launch)
        );
        assert_eq!(
            fs::metadata(fixture.0.join("harnesses.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(
            create_custom(
                &fixture.0,
                HarnessLaunch {
                    name: "Different".into(),
                    ..launch.clone()
                }
            )
            .is_err()
        );
        let updated = HarnessLaunch {
            name: "Renamed".into(),
            ..launch.clone()
        };
        assert_eq!(update_custom(&fixture.0, updated.clone()).unwrap(), updated);
        remove_custom(&fixture.0, &launch.id).unwrap();
        remove_custom(&fixture.0, &launch.id).unwrap();
        assert!(custom_launches(&fixture.0).unwrap().is_empty());

        for wrapper in ["/usr/bin/env", "/bin/sh", "/bin/zsh"] {
            let invalid = HarnessLaunch {
                id: "isolated".into(),
                name: "Isolated".into(),
                command: wrapper.into(),
                args: vec!["HOME=/tmp/empty".into(), "agent".into()],
                version: "1".into(),
                environment: None,
            };
            assert!(create_custom(&fixture.0, invalid).is_err());
        }
    }

    #[test]
    fn custom_registry_never_follows_a_symlink() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let target = outside.0.join("registry.json");
        fs::write(&target, b"[]").unwrap();
        std::os::unix::fs::symlink(&target, fixture.0.join("harnesses.json")).unwrap();
        assert!(custom_launches(&fixture.0).is_err());
        assert_eq!(fs::read(target).unwrap(), b"[]");
    }
}
