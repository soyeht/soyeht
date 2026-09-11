use crate::wire::{Instance, VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    pub engine_generation: u64,
    pub instances: BTreeMap<String, Instance>,
}

impl Registry {
    pub fn load(directory: &Path) -> io::Result<Self> {
        let path = directory.join("ptyd-registry.json");
        let mut registry = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self {
                version: VERSION,
                engine_generation: 0,
                instances: BTreeMap::new(),
            },
            Err(error) => return Err(error),
        };
        if registry.version != VERSION {
            return Err(io::Error::other("incompatible registry version"));
        }
        // Never signal or adopt a process by a PID from a prior supervisor.
        for instance in registry.instances.values_mut() {
            instance.closed = true;
        }
        registry.save(directory)?;
        Ok(registry)
    }

    pub fn save(&self, directory: &Path) -> io::Result<()> {
        let temporary = directory.join("ptyd-registry.new");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.sync_all()?;
        fs::rename(temporary, directory.join("ptyd-registry.json"))?;
        File::open(directory)?.sync_all()
    }
}
