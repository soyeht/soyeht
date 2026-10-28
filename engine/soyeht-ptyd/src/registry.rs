use crate::{Error, Result, wire::Instance};
use serde::{Deserialize, Serialize};
use soyeht_platform::fs::{WriteMode, write_atomic};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    pub engine_generation: u64,
    #[serde(default)]
    pub supervisor_generation: u32,
    pub instances: BTreeMap<String, Instance>,
}

impl Registry {
    pub fn load(directory: &Path) -> Result<Self> {
        let path = directory.join("ptyd-registry.json");
        let mut registry = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes)?,
            Err(error) if error.kind() == ErrorKind::NotFound => Self {
                version: 1,
                engine_generation: 0,
                supervisor_generation: 0,
                instances: BTreeMap::new(),
            },
            Err(error) => return Err(error.into()),
        };
        if registry.version != 1 {
            return Err(Error::IncompatibleRegistryVersion);
        }
        registry.supervisor_generation = registry
            .supervisor_generation
            .checked_add(1)
            .ok_or(Error::SupervisorGenerationExhausted)?;
        // Never signal or adopt a process by a PID from a prior supervisor.
        for instance in registry.instances.values_mut() {
            instance.closed = true;
        }
        registry.save(directory)?;
        Ok(registry)
    }

    pub fn save(&self, directory: &Path) -> Result<()> {
        write_atomic(
            &directory.join("ptyd-registry.json"),
            &serde_json::to_vec(self)?,
            WriteMode::OwnerOnly,
        )?;
        Ok(())
    }
}
