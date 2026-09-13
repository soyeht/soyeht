//! Private MCP declaration supplied to production ACP sessions.

use super::state::State;
use crate::{
    errors::{self, Result},
    model::StoredPane,
    tasks::ports::TaskStorePort,
};
use soyeht_acp::client::{EnvVariable, StdioMcpServer};
use std::path::{Path, PathBuf};

const BINDING_ENV: &str = "SOYEHT_MCP_BINDING";

pub fn declaration(
    store: &impl TaskStorePort,
    data_directory: &Path,
    state: &State,
    saved: &StoredPane,
) -> Result<StdioMcpServer> {
    let executable = sibling_executable()?;
    declaration_with_executable(store, data_directory, &executable, state, saved)
}

fn sibling_executable() -> Result<PathBuf> {
    Ok(std::env::current_exe()
        .map_err(errors::io)?
        .with_file_name("soyeht-mcp"))
}

fn declaration_with_executable(
    store: &impl TaskStorePort,
    data_directory: &Path,
    executable: &Path,
    state: &State,
    saved: &StoredPane,
) -> Result<StdioMcpServer> {
    if !data_directory.is_absolute() || !executable.is_absolute() {
        return Err(errors::internal("MCP paths must be absolute"));
    }
    let mut args = vec![
        "--data-dir".into(),
        path_text(data_directory)?,
        "--workspace-id".into(),
        saved.pane.workspace_id.clone(),
        "--pane-id".into(),
        saved.pane.pane_id.clone(),
        "--session-generation".into(),
        saved.pane.generation.to_string(),
    ];
    if let Some(task) = store
        .active_task_for_pane(&saved.pane.pane_id)
        .map_err(errors::storage)?
    {
        if task.workspace_id != saved.pane.workspace_id || task.current_attempt == 0 {
            return Err(errors::internal("Active task binding is inconsistent"));
        }
        args.extend([
            "--task-id".into(),
            task.task_id,
            "--attempt".into(),
            task.current_attempt.to_string(),
        ]);
    }
    Ok(StdioMcpServer {
        name: "Soyeht".into(),
        command: path_text(executable)?,
        args,
        env: vec![EnvVariable {
            name: BINDING_ENV.into(),
            value: state.binding.expose().into(),
        }],
    })
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| errors::internal("MCP path must be UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{ports::testing::MemoryTaskStore, task_fixture};
    use soyeht_protocol::{
        HarnessLaunch, Pane, PaneMode, PaneState, PermissionMode, StateSource, TaskState,
    };
    use soyeht_store::domain::PaneRecord;

    fn fixture() -> (MemoryTaskStore, State, PaneRecord) {
        let state = State::new(
            HarnessLaunch {
                id: "test".into(),
                name: "Test".into(),
                command: "/bin/false".into(),
                args: Vec::new(),
                version: "1".into(),
            },
            "digest".into(),
            PermissionMode::Ask,
        )
        .unwrap();
        let saved = PaneRecord {
            pane: Pane {
                pane_id: "pane-1".into(),
                workspace_id: "workspace-1".into(),
                name: "Agent".into(),
                cwd: "/tmp/worktree".into(),
                mode: PaneMode::Agent,
                state: PaneState::Idle,
                source: StateSource::Protocol,
                confidence: 1.0,
                phrase: "Ready".into(),
                generation: 7,
                cols: 80,
                rows: 24,
                record: true,
                pid: None,
                stream_id: None,
                agent: None,
                url: None,
                title: None,
                viewport: None,
                incognito: None,
                web_session: None,
            },
            closed: false,
            instance_key: "instance".into(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        (MemoryTaskStore::default(), state, saved)
    }

    #[test]
    fn declaration_keeps_the_secret_out_of_argv_and_binds_the_active_attempt() {
        let (mut store, state, saved) = fixture();
        let mut task = task_fixture(TaskState::InProgress);
        task.workspace_id = saved.pane.workspace_id.clone();
        task.owner_pane_id = Some(saved.pane.pane_id.clone());
        task.current_attempt = 3;
        store
            .tasks
            .insert((task.workspace_id.clone(), task.task_id.clone()), task);

        let declaration = declaration_with_executable(
            &store,
            Path::new("/tmp/data"),
            Path::new("/opt/soyeht/bin/soyeht-mcp"),
            &state,
            &saved,
        )
        .unwrap();

        assert_eq!(declaration.name, "Soyeht");
        assert_eq!(declaration.command, "/opt/soyeht/bin/soyeht-mcp");
        assert_eq!(declaration.env[0].name, BINDING_ENV);
        assert_eq!(declaration.env[0].value, state.binding.expose());
        assert!(
            !declaration
                .args
                .iter()
                .any(|arg| arg == state.binding.expose())
        );
        assert!(
            declaration
                .args
                .windows(2)
                .any(|pair| pair == ["--task-id", "WS-1"])
        );
        assert!(
            declaration
                .args
                .windows(2)
                .any(|pair| pair == ["--attempt", "3"])
        );
    }

    #[test]
    fn declaration_without_an_active_task_has_only_the_pane_identity() {
        let (store, state, saved) = fixture();
        let declaration = declaration_with_executable(
            &store,
            Path::new("/tmp/data"),
            Path::new("/opt/soyeht/bin/soyeht-mcp"),
            &state,
            &saved,
        )
        .unwrap();
        assert!(!declaration.args.iter().any(|arg| arg == "--task-id"));
        assert!(!declaration.args.iter().any(|arg| arg == "--attempt"));
    }
}
