use crate::{
    agents::{
        hooks::{CLAUDE_HARNESS_ID, terminal_hook_settings},
        registry,
        state::Binding,
    },
    errors::{self, Result},
};
use serde::{Deserialize, Serialize};
use soyeht_pane_state::HarnessIdentity;
use soyeht_protocol::HarnessLaunch;
use soyeht_ptyd::wire::{ProcessKind, Spawn};
use std::path::{Path, PathBuf};

const LAUNCH_TYPE: &str = "terminal_harness";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalHarnessSession {
    launch_type: String,
    pub(crate) spawn: Spawn,
    pub(crate) harness: HarnessIdentity,
    pub(crate) hook_binding: Binding,
}

impl TerminalHarnessSession {
    pub(crate) fn prepare(
        data_directory: &Path,
        harness_id: &str,
        instance_key: String,
        generation: u32,
        cwd: String,
        cols: u16,
        rows: u16,
    ) -> Result<(Self, String)> {
        if harness_id != CLAUDE_HARNESS_ID {
            return Err(errors::harness_unavailable(
                "Harness has no session-scoped terminal hook integration",
            ));
        }
        let (launch, command) = registry::prepare_local_terminal(data_directory, harness_id)?;
        Self::prepare_resolved(
            data_directory,
            launch,
            command,
            instance_key,
            generation,
            cwd,
            cols,
            rows,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_resolved(
        data_directory: &Path,
        launch: HarnessLaunch,
        command: PathBuf,
        instance_key: String,
        generation: u32,
        cwd: String,
        cols: u16,
        rows: u16,
    ) -> Result<(Self, String)> {
        let hook_binding = Binding::generate()?;
        let settings = terminal_hook_settings(data_directory, &hook_binding)?;
        let settings = serde_json::to_string(&settings)
            .map_err(|_| errors::internal("Terminal hook settings could not be encoded"))?;
        let command = command
            .to_str()
            .ok_or_else(|| errors::internal("Terminal harness path must be UTF-8"))?
            .to_owned();
        let harness = HarnessIdentity::known(launch.id, None)?;
        Ok((
            Self {
                launch_type: LAUNCH_TYPE.into(),
                spawn: Spawn {
                    instance_key,
                    generation,
                    kind: ProcessKind::Pty,
                    cwd,
                    argv: vec![command, "--settings".into(), settings],
                    env: Default::default(),
                    cols,
                    rows,
                },
                harness,
                hook_binding,
            },
            launch.name,
        ))
    }

    pub(crate) fn decode(value: serde_json::Value) -> Result<Option<Self>> {
        if value.get("launchType").and_then(serde_json::Value::as_str) != Some(LAUNCH_TYPE) {
            return Ok(None);
        }
        let session: Self = serde_json::from_value(value)
            .map_err(|_| errors::internal("Stored terminal harness session is invalid"))?;
        session.validate()?;
        Ok(Some(session))
    }

    fn validate(&self) -> Result<()> {
        if self.launch_type != LAUNCH_TYPE
            || self.spawn.kind != ProcessKind::Pty
            || self.spawn.generation == 0
            || self.hook_binding.expose().is_empty()
        {
            return Err(errors::internal(
                "Stored terminal harness session is invalid",
            ));
        }
        self.harness.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn launch() -> HarnessLaunch {
        HarnessLaunch {
            id: CLAUDE_HARNESS_ID.into(),
            name: "Claude Agent".into(),
            command: "npx".into(),
            args: vec![],
            version: "adapter-version".into(),
            environment: None,
        }
    }

    #[test]
    fn resolved_terminal_session_scopes_hooks_and_round_trips() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-terminal-harness-{}", uuid::Uuid::new_v4()));
        let (session, name) = TerminalHarnessSession::prepare_resolved(
            &directory,
            launch(),
            PathBuf::from("/test/bin/claude"),
            "instance-one".into(),
            7,
            "/workspace".into(),
            120,
            40,
        )
        .unwrap();

        assert_eq!(name, "Claude Agent");
        assert_eq!(session.spawn.argv[0], "/test/bin/claude");
        assert_eq!(session.spawn.argv[1], "--settings");
        let settings: Value = serde_json::from_str(&session.spawn.argv[2]).unwrap();
        let handler = &settings["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert_eq!(handler["args"][0], "--data-dir");
        assert_eq!(handler["args"][1], directory.to_string_lossy().as_ref());
        assert_eq!(handler["args"][2], "__experiment-hook");
        assert_eq!(handler["args"][3], session.hook_binding.expose());
        assert_eq!(session.harness.id(), Some(CLAUDE_HARNESS_ID));
        assert_eq!(session.spawn.generation, 7);

        let encoded = serde_json::to_value(&session).unwrap();
        let decoded = TerminalHarnessSession::decode(encoded).unwrap().unwrap();
        assert_eq!(decoded.spawn.argv, session.spawn.argv);
        assert_eq!(decoded.harness, session.harness);
        assert!(decoded.hook_binding.matches(session.hook_binding.expose()));
    }

    #[test]
    fn terminal_session_rejects_a_harness_without_hook_integration() {
        let error = TerminalHarnessSession::prepare(
            Path::new("/unused"),
            "registry.unsupported",
            "instance".into(),
            1,
            "/workspace".into(),
            80,
            24,
        )
        .err()
        .unwrap();
        assert!(matches!(error, crate::errors::Error::HarnessUnavailable(_)));
    }
}
