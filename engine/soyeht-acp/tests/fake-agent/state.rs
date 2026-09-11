use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::PathBuf};
use uuid::Uuid;

#[derive(Clone)]
pub struct Settings {
    pub directory: PathBuf,
    pub load: String,
    pub version: String,
    pub require_login: bool,
    pub login: bool,
}
impl Settings {
    pub fn parse() -> Result<Self> {
        let mut directory = None;
        let mut load = "supported".to_owned();
        let mut version = "1".to_owned();
        let mut require_login = false;
        let mut login = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--state-dir" => directory = args.next().map(PathBuf::from),
                "--load" => load = args.next().ok_or(Error::MissingLoadPolicy)?,
                "--version-label" => version = args.next().ok_or(Error::MissingVersionLabel)?,
                "--require-login" => require_login = true,
                "--login" => login = true,
                _ => return Err(Error::UnknownOption(arg)),
            }
        }
        let directory = directory
            .filter(|p| p.is_absolute())
            .ok_or(Error::InvalidStateDirectory)?;
        if !matches!(
            load.as_str(),
            "supported" | "unsupported" | "advertised-only" | "missing" | "timeout"
        ) {
            return Err(Error::UnknownLoadPolicy);
        }
        Ok(Self {
            directory,
            load,
            version,
            require_login,
            login,
        })
    }
    pub fn logged_in(&self) -> bool {
        !self.require_login || self.directory.join("login.ok").exists()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub cwd: String,
    pub values: BTreeMap<String, Value>,
    pub history: Vec<(String, String)>,
}
impl Session {
    pub fn new(cwd: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            cwd,
            history: Vec::new(),
            values: BTreeMap::from([
                ("model".into(), json!("model-a")),
                ("effort".into(), json!("medium")),
                ("mode".into(), json!("ask")),
                ("fast".into(), json!(false)),
            ]),
        }
    }
    pub fn options(&self) -> Value {
        let select = |id: &str, name: &str, category: &str, values: &[&str]| json!({"id":id,"name":name,"category":category,"type":"select","currentValue":self.values[id],"options":values.iter().map(|value|json!({"value":value,"name":value})).collect::<Vec<_>>()});
        json!([
            select("model","Model","model",&["model-a","model-b","rejected-model","delayed-model"]),
            select("effort","Effort","thought_level",&["low","medium","high"]),
            select("mode","Permission","mode",&["ask","accept_edits","allow_all"]),
            {"id":"fast","name":"Fast","type":"boolean","currentValue":self.values["fast"]}
        ])
    }
    pub fn response(&self) -> Value {
        json!({"sessionId":self.id,"configOptions":self.options()})
    }
    pub fn save(&self, settings: &Settings) -> Result<()> {
        fs::create_dir_all(&settings.directory)?;
        fs::set_permissions(&settings.directory, fs::Permissions::from_mode(0o700))?;
        let temporary = settings.directory.join(format!("{}.tmp", self.id));
        fs::write(&temporary, serde_json::to_vec(self)?)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        fs::File::open(&temporary)?.sync_all()?;
        fs::rename(
            temporary,
            settings.directory.join(format!("{}.json", self.id)),
        )?;
        fs::File::open(&settings.directory)?.sync_all()?;
        Ok(())
    }
    pub fn load(settings: &Settings, id: &str) -> Result<Self> {
        Uuid::parse_str(id).map_err(|_| Error::InvalidSessionId)?;
        Ok(serde_json::from_slice(&fs::read(
            settings.directory.join(format!("{id}.json")),
        )?)?)
    }
}
use crate::{error::Error, error::Result};
