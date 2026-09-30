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
    pub initialize_padding_bytes: usize,
    /// Replays each user prompt trimmed and split into chunks of this many
    /// bytes under one messageId, as real adapters do for long prompts.
    pub replay_user_chunk_bytes: Option<usize>,
    /// How permission modes are announced: `config` as a configuration
    /// option, `legacy` as ACP session modes, `none` not at all.
    pub modes: String,
}
impl Settings {
    pub fn parse() -> Result<Self> {
        let mut directory = None;
        let mut load = "supported".to_owned();
        let mut version = "1".to_owned();
        let mut require_login = false;
        let mut login = false;
        let mut initialize_padding_bytes = 0;
        let mut replay_user_chunk_bytes = None;
        let mut modes = "config".to_owned();
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--state-dir" => directory = args.next().map(PathBuf::from),
                "--load" => load = args.next().ok_or(Error::MissingLoadPolicy)?,
                "--version-label" => version = args.next().ok_or(Error::MissingVersionLabel)?,
                "--require-login" => require_login = true,
                "--login" => login = true,
                "--modes" => {
                    modes = args
                        .next()
                        .filter(|value| matches!(value.as_str(), "config" | "legacy" | "none"))
                        .ok_or_else(|| Error::UnknownOption(arg.clone()))?;
                }
                "--initialize-padding-bytes" => {
                    let value = args
                        .next()
                        .ok_or_else(|| Error::UnknownOption(arg.clone()))?;
                    initialize_padding_bytes = value
                        .parse()
                        .ok()
                        .filter(|bytes| *bytes <= 128 * 1024)
                        .ok_or(Error::UnknownOption(value))?;
                }
                "--replay-user-chunk-bytes" => {
                    let value = args
                        .next()
                        .ok_or_else(|| Error::UnknownOption(arg.clone()))?;
                    replay_user_chunk_bytes = Some(
                        value
                            .parse()
                            .ok()
                            .filter(|bytes| *bytes > 0)
                            .ok_or(Error::UnknownOption(value))?,
                    );
                }
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
            initialize_padding_bytes,
            replay_user_chunk_bytes,
            modes,
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
    #[serde(default)]
    pub mcp_servers: Vec<Value>,
    #[serde(default)]
    pub session_meta: Option<Value>,
}
impl Session {
    pub fn new(cwd: String, mcp_servers: Vec<Value>, session_meta: Option<Value>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            cwd,
            history: Vec::new(),
            mcp_servers,
            session_meta,
            values: BTreeMap::from([
                ("model".into(), json!("model-a")),
                ("effort".into(), json!("medium")),
                ("mode".into(), json!("ask")),
                ("fast".into(), json!(false)),
            ]),
        }
    }
    pub fn options(&self, settings: &Settings) -> Value {
        let select = |id: &str, name: &str, category: &str, values: &[&str]| json!({"id":id,"name":name,"category":category,"type":"select","currentValue":self.values[id],"options":values.iter().map(|value|json!({"value":value,"name":value})).collect::<Vec<_>>()});
        let mut options = vec![
            select(
                "model",
                "Model",
                "model",
                &["model-a", "model-b", "rejected-model", "delayed-model"],
            ),
            select(
                "effort",
                "Effort",
                "thought_level",
                &["low", "medium", "high"],
            ),
            json!({"id":"fast","name":"Fast","type":"boolean","currentValue":self.values["fast"]}),
        ];
        if settings.modes == "config" {
            let modes: Vec<_> = MODES
                .iter()
                .map(|(value, name, kind)| json!({"value":value,"name":name,"_meta":{"kind":kind}}))
                .collect();
            options.insert(2, json!({"id":"mode","name":"Permission","category":"mode","type":"select","currentValue":self.values["mode"],"options":modes}));
        }
        json!(options)
    }
    /// A session/new or session/load result in the selected announcement style.
    pub fn response(&self, settings: &Settings) -> Value {
        let mut response = json!({"sessionId":self.id,"configOptions":self.options(settings)});
        if settings.modes == "legacy" {
            let modes: Vec<_> = MODES
                .iter()
                .map(|(id, name, kind)| json!({"id":id,"name":name,"_meta":{"kind":kind}}))
                .collect();
            response["modes"] = json!({"currentModeId":self.values["mode"],"availableModes":modes});
        }
        response
    }
    pub fn mode(&self) -> &str {
        self.values["mode"].as_str().unwrap_or_default()
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

/// Announced permission modes. The agent approves everything itself in
/// `allow_all` and always refuses to enter `locked`.
pub const MODES: [(&str, &str, &str); 4] = [
    ("ask", "Ask", "standard"),
    ("accept_edits", "Accept edits", "standard"),
    ("allow_all", "Allow all", "full_access"),
    ("locked", "Locked", "standard"),
];
