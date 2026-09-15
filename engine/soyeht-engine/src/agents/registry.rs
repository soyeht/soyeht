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
}
