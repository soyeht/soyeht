use crate::errors::{self, Result};
use sha2::{Digest, Sha256};
use soyeht_protocol::{HarnessLaunch, limits};
use std::{
    collections::BTreeSet,
    fs::File,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// This file belongs to the local profile. Remote pane creation can select an
/// entry by ID but cannot supply a command or change this launch configuration.
pub fn entries(directory: &Path) -> Result<Vec<HarnessLaunch>> {
    let file = match File::open(directory.join("harnesses.json")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(errors::io(error)),
    };
    let mut bytes = Vec::new();
    file.take(limits::JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(errors::io)?;
    if bytes.len() > limits::JSON_BYTES {
        return Err(errors::invalid("Harness registry exceeds 1 MiB"));
    }
    let entries: Vec<HarnessLaunch> =
        serde_json::from_slice(&bytes).map_err(|_| errors::invalid("Invalid harness registry"))?;
    let mut seen = BTreeSet::new();
    if entries.len() > 64
        || entries.iter().any(|entry| {
            entry.id.is_empty()
                || entry.id.chars().count() > 128
                || !seen.insert(entry.id.clone())
                || entry.command.is_empty()
                || entry.command.contains('\0')
                || entry.args.iter().any(|arg| arg.contains('\0'))
        })
    {
        return Err(errors::invalid("Invalid or duplicate harness launch entry"));
    }
    Ok(entries)
}

pub fn executable(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    let candidates = if path.is_absolute() {
        vec![path.to_owned()]
    } else if path.components().count() == 1 {
        std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths)
                    .map(|directory| directory.join(command))
                    .collect()
            })
            .unwrap_or_default()
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

pub fn prepare(directory: &Path, id: &str) -> Result<(HarnessLaunch, String)> {
    let mut entry = entries(directory)?
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| errors::harness_unavailable("Harness is not configured in this profile"))?;
    let path = executable(&entry.command)
        .ok_or_else(|| errors::harness_unavailable("Harness executable is unavailable"))?;
    let mut file = File::open(&path).map_err(errors::io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(errors::io)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    entry.command = path
        .to_str()
        .ok_or_else(|| errors::harness_unavailable("Harness path is not UTF-8"))?
        .into();
    Ok((entry, format!("{:x}", digest.finalize())))
}
