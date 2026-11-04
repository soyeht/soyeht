//! OpenCode: one meter for a program that talks to many providers. The meter follows the
//! provider OpenCode is set to (`model` in its configuration, else the model it used last), and
//! reads that provider's own plan when the provider has one a reader exists for:
//!
//! 1. the Go allowance, OpenCode's own subscription (provider `opencode` or `opencode-go`);
//! 2. the Z.ai or Zhipu GLM Coding Plan quota (provider `zai-coding-plan`, `zhipuai-coding-plan`);
//! 3. otherwise, or when there is no plan to read, the tokens and cost OpenCode recorded on this
//!    device. That is activity, not a quota: it has no window and no percentage.
//!
//! Everything is read-only. A token travels only to its own provider's endpoint. The sources
//! and their status (documented, experimental, vendor plugin) are in contract 10 §3.

mod go;
mod local;
mod zai;

use super::{Failure, Source, read_json_file};
use crate::host::{Host, Secret, StoredCredential};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) fn chain() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(go::GoAllowanceSource),
        Box::new(zai::ZaiPlanSource),
        Box::new(local::LocalHistorySource),
    ]
}

/// A directory of the XDG layout OpenCode uses on every system: the variable when it is an
/// absolute path, else `default` under the home.
fn xdg_dir(host: &dyn Host, variable: &str, default: &str) -> Option<PathBuf> {
    host.env(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| host.home().map(|home| home.join(default)))
        .map(|base| base.join("opencode"))
}

pub(super) fn data_dir(host: &dyn Host) -> Option<PathBuf> {
    xdg_dir(host, "XDG_DATA_HOME", ".local/share")
}

/// OpenCode's database: `$OPENCODE_DB` (a file name under the data directory, or an absolute
/// path; `:memory:` has no file), else `opencode.db` in the data directory.
pub(super) fn database(host: &dyn Host) -> Option<PathBuf> {
    let data = data_dir(host)?;
    match host.env("OPENCODE_DB").map(|value| value.trim().to_owned()) {
        Some(name) if name == ":memory:" => None,
        Some(name) if !name.is_empty() => {
            let path = PathBuf::from(&name);
            Some(if path.is_absolute() {
                path
            } else {
                data.join(path)
            })
        }
        _ => Some(data.join("opencode.db")),
    }
}

/// The provider OpenCode is set to: the `model` of its configuration (`provider/model`), else
/// the provider of the model it used last. A configuration that is not plain JSON is left
/// unread, so the last used model decides.
pub(super) fn active_provider(host: &dyn Host) -> Option<String> {
    configured_provider(host).or_else(|| last_used_provider(host))
}

fn configured_provider(host: &dyn Host) -> Option<String> {
    let path = match host
        .env("OPENCODE_CONFIG")
        .filter(|path| !path.trim().is_empty())
    {
        Some(path) => PathBuf::from(path.trim()),
        None => xdg_dir(host, "XDG_CONFIG_HOME", ".config")?.join("opencode.json"),
    };
    // ignored: a configuration that is missing or not plain JSON leaves the last used model to decide
    let config = read_json_file(host, &path).ok()?;
    let model = config.get("model")?.as_str()?;
    provider_of(model)
}

fn last_used_provider(host: &dyn Host) -> Option<String> {
    let path = xdg_dir(host, "XDG_STATE_HOME", ".local/state")?.join("model.json");
    // ignored: with no readable state either, the meter reads the device activity of every provider
    let state = read_json_file(host, &path).ok()?;
    let recent = state.get("recent")?.as_array()?.first()?;
    let provider = recent.get("providerID")?.as_str()?;
    (!provider.is_empty() && provider.len() <= 128).then(|| provider.to_owned())
}

fn provider_of(model: &str) -> Option<String> {
    let (provider, _) = model.trim().split_once('/')?;
    (!provider.is_empty() && provider.len() <= 128).then(|| provider.to_owned())
}

/// Whether OpenCode is set to one of `providers`.
pub(super) fn follows(host: &dyn Host, providers: &[&str]) -> bool {
    active_provider(host).is_some_and(|active| providers.contains(&active.as_str()))
}

/// A login OpenCode holds for one integration.
pub(super) struct Credential {
    pub secret: Secret,
    /// The Console workspace the login chose (Go usage is asked for it).
    pub org_id: Option<String>,
    /// The Console the login was made against, when it is not the default.
    pub server: Option<String>,
}

/// The login OpenCode holds for `integration`: the active row of its credential table, else, when
/// that table holds no credential at all, the `auth.json` entry. A table that holds credentials
/// but none for the integration is the answer: a leftover `auth.json` is not used in its place.
/// `NotConfigured` means there is no login.
pub(super) fn credential(host: &dyn Host, integration: &str) -> Result<Credential, Failure> {
    let stored = match database(host) {
        Some(database) => host.opencode_credential(&database, integration),
        None => StoredCredential::NoTable,
    };
    match stored {
        StoredCredential::Found(text) => {
            let value: Value =
                serde_json::from_str(text.expose()).map_err(|_| Failure::InvalidResponse)?;
            parse_credential(&value)
        }
        StoredCredential::NotInTable => Err(Failure::NotConfigured),
        StoredCredential::Unreadable => Err(Failure::Failed),
        StoredCredential::NoTable => {
            let path = data_dir(host)
                .ok_or(Failure::NotConfigured)?
                .join("auth.json");
            let auth = read_json_file(host, &path)?;
            parse_credential(auth.get(integration).ok_or(Failure::NotConfigured)?)
        }
    }
}

/// A key (`key`, stored by older versions as `api`) or an OAuth login (`access`).
fn parse_credential(value: &Value) -> Result<Credential, Failure> {
    let field = match value.get("type").and_then(Value::as_str) {
        Some("key" | "api") => "key",
        Some("oauth") => "access",
        _ => return Err(Failure::NotConfigured),
    };
    let secret = value
        .get(field)
        .and_then(Value::as_str)
        .map(|token| Secret::new(token.trim()))
        .filter(Secret::is_usable)
        .ok_or(Failure::NotConfigured)?;
    let metadata = |name: &str| {
        value
            .get("metadata")
            .and_then(|metadata| metadata.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Ok(Credential {
        secret,
        org_id: metadata("orgID"),
        server: metadata("server"),
    })
}

/// Whether OpenCode is on this machine: its program, its database or its data directory.
pub(super) fn installed(host: &dyn Host) -> bool {
    host.program_found("opencode")
        || database(host).is_some_and(|path| exists(host, &path))
        || data_dir(host).is_some_and(|path| exists(host, &path.join("auth.json")))
}

/// Whether a file or directory is there. The one-byte bound makes a file of any size but empty
/// read as `Unreadable` and a directory as not a file: either way it is not `Missing`.
fn exists(host: &dyn Host, path: &Path) -> bool {
    !matches!(host.read_file(path, 1), crate::host::FileRead::Missing)
}
