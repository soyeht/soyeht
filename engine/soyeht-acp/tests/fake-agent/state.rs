use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Restricts a state file or folder to its owner on systems that have permission modes; the
/// fake agent's state lives in a temporary folder that is private to the account elsewhere.
fn private(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}
use uuid::Uuid;

#[derive(Clone)]
pub struct Settings {
    pub directory: PathBuf,
    pub load: String,
    pub version: String,
    pub require_login: bool,
    pub login: bool,
    /// Like Cursor: every process asks to be authenticated with the protocol sign-in
    /// `existing-login` before it opens a session, and the person's `login.ok` is what
    /// that request checks.
    pub session_auth: bool,
    /// Advertises and serves the ACP `logout` request.
    pub logout: bool,
    pub initialize_padding_bytes: usize,
    /// Replays each user prompt trimmed and split into chunks of this many
    /// bytes under one messageId, as real adapters do for long prompts.
    pub replay_user_chunk_bytes: Option<usize>,
    /// How permission modes are announced: `config` as a configuration
    /// option, `restricted` the same without full access, `legacy` as ACP
    /// session modes, `none` not at all.
    pub modes: String,
    /// The `promptCapabilities` flags the agent advertises. The default
    /// matches the pinned Claude and Codex adapters: image and embeddedContext.
    pub prompt_capabilities: Vec<String>,
}
impl Settings {
    pub fn parse() -> Result<Self> {
        let mut directory = None;
        let mut load = "supported".to_owned();
        let mut version = "1".to_owned();
        let mut require_login = false;
        let mut login = false;
        let mut session_auth = false;
        let mut logout = false;
        let mut initialize_padding_bytes = 0;
        let mut replay_user_chunk_bytes = None;
        let mut modes = "config".to_owned();
        let mut prompt_capabilities = vec!["image".to_owned(), "embeddedContext".to_owned()];
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--state-dir" => directory = args.next().map(PathBuf::from),
                "--load" => load = args.next().ok_or(Error::MissingLoadPolicy)?,
                "--version-label" => version = args.next().ok_or(Error::MissingVersionLabel)?,
                "--require-login" => require_login = true,
                "--login" => login = true,
                "--session-auth" => session_auth = true,
                "--logout" => logout = true,
                "--modes" => {
                    modes = args
                        .next()
                        .filter(|value| {
                            matches!(value.as_str(), "config" | "restricted" | "legacy" | "none")
                        })
                        .ok_or_else(|| Error::UnknownOption(arg.clone()))?;
                }
                "--prompt-capabilities" => {
                    let value = args
                        .next()
                        .ok_or_else(|| Error::UnknownOption(arg.clone()))?;
                    prompt_capabilities = value
                        .split(',')
                        .filter(|flag| !flag.is_empty() && *flag != "none")
                        .map(str::to_owned)
                        .collect();
                    if prompt_capabilities
                        .iter()
                        .any(|flag| !matches!(flag.as_str(), "image" | "audio" | "embeddedContext"))
                    {
                        return Err(Error::UnknownOption(value));
                    }
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
            session_auth,
            logout,
            initialize_padding_bytes,
            replay_user_chunk_bytes,
            modes,
            prompt_capabilities,
        })
    }
    pub fn announces_mode_option(&self) -> bool {
        matches!(self.modes.as_str(), "config" | "restricted")
    }
    /// Whether this agent offers `mode`; it never agrees to enter `locked`.
    pub fn offers_mode(&self, mode: &str) -> bool {
        self.modes != "none" && (self.modes != "restricted" || mode != "allow_all")
    }
    pub fn accepts_mode(&self, mode: &str) -> bool {
        self.offers_mode(mode)
            && MODES
                .iter()
                .any(|(value, _, _)| *value == mode && mode != "locked")
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
    /// Non-text prompt blocks by turn, replayed after that turn's text the
    /// way the Claude adapter replays images.
    #[serde(default)]
    pub attached: BTreeMap<usize, Vec<Value>>,
    #[serde(default)]
    pub mcp_servers: Vec<Value>,
    #[serde(default)]
    pub session_meta: Option<Value>,
    #[serde(default)]
    pub additional_directories: Vec<String>,
}
impl Session {
    pub fn new(cwd: String, mcp_servers: Vec<Value>, session_meta: Option<Value>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            cwd,
            history: Vec::new(),
            attached: BTreeMap::new(),
            mcp_servers,
            session_meta,
            additional_directories: Vec::new(),
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
                &[
                    "model-a",
                    "model-b",
                    "rejected-model",
                    "delayed-model",
                    "mode-shifting-model",
                ],
            ),
            select(
                "effort",
                "Effort",
                "thought_level",
                &["low", "medium", "high"],
            ),
            json!({"id":"fast","name":"Fast","type":"boolean","currentValue":self.values["fast"]}),
        ];
        if settings.announces_mode_option() {
            let modes: Vec<_> = MODES
                .iter()
                .filter(|(value, _, _)| settings.offers_mode(value))
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
        private(&settings.directory, 0o700)?;
        let temporary = settings.directory.join(format!("{}.tmp", self.id));
        fs::write(&temporary, serde_json::to_vec(self)?)?;
        private(&temporary, 0o600)?;
        fs::OpenOptions::new()
            .write(true)
            .open(&temporary)?
            .sync_all()?;
        fs::rename(
            temporary,
            settings.directory.join(format!("{}.json", self.id)),
        )?;
        // A folder cannot be opened for flushing on Windows, whose file system journals renames.
        #[cfg(unix)]
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
