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
/// A name no user-level MCP configuration shares, so a harness that also
/// loads the user's own Soyeht servers still routes this pane's tools here.
pub(crate) const SERVER_NAME: &str = "soyeht-workspace";

pub fn declaration(
    store: &impl TaskStorePort,
    data_directory: &Path,
    state: &State,
    saved: &StoredPane,
) -> Result<StdioMcpServer> {
    let executable = installed(sibling_executable("soyeht-mcp")?)?;
    declaration_with_executable(store, data_directory, &executable, state, saved)
}

/// The program called `name` that is built and installed next to the engine; Windows adds `.exe`.
pub(super) fn sibling_executable(name: &str) -> Result<PathBuf> {
    Ok(std::env::current_exe()
        .map_err(errors::io)?
        .with_file_name(name)
        .with_extension(std::env::consts::EXE_EXTENSION))
}

/// `program` when it exists. An agent never starts with tools its harness could not launch: a
/// missing `soyeht-mcp` is reported here, where the cause is known, instead of as a harness
/// that silently lacks its tools.
fn installed(program: PathBuf) -> Result<PathBuf> {
    if program.is_file() {
        return Ok(program);
    }
    Err(errors::internal(format!(
        "{} is not installed next to the engine; build the whole workspace",
        program.display()
    )))
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
        name: SERVER_NAME.into(),
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
    use soyeht_protocol::{HarnessLaunch, Pane, PaneMode, PaneState, StateSource, TaskState};
    use soyeht_store::domain::PaneRecord;

    fn fixture() -> (MemoryTaskStore, State, PaneRecord) {
        let state = State::new(
            HarnessLaunch {
                id: "test".into(),
                name: "Test".into(),
                command: "/bin/false".into(),
                args: Vec::new(),
                version: "1".into(),
                environment: None,
            },
            "digest".into(),
            None,
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
                experiment_capture: None,
                pid: None,
                stream_id: None,
                agent: None,
                url: None,
                title: None,
                viewport: None,
                incognito: None,
                writing_kind: None,
                writing_section: None,
                exit_code: None,
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

    fn declare(store: &MemoryTaskStore, state: &State, saved: &PaneRecord) -> StdioMcpServer {
        declaration_with_executable(
            store,
            &crate::test_paths::rooted("/tmp/data"),
            &crate::test_paths::rooted("/opt/soyeht/bin/soyeht-mcp"),
            state,
            saved,
        )
        .unwrap()
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

        let declaration = declare(&store, &state, &saved);

        assert_eq!(declaration.name, SERVER_NAME);
        assert_eq!(
            declaration.command,
            crate::test_paths::rooted_text("/opt/soyeht/bin/soyeht-mcp")
        );
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
    fn a_program_is_used_only_when_it_is_installed() {
        let folder = std::env::temp_dir().join(format!("mcp-installed-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mcp = folder.join(format!("soyeht-mcp{}", std::env::consts::EXE_SUFFIX));
        let missing = installed(mcp.clone()).unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("is not installed next to the engine")
        );

        std::fs::write(&mcp, b"program").unwrap();
        assert_eq!(installed(mcp.clone()).unwrap(), mcp);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn declaration_without_an_active_task_has_only_the_pane_identity() {
        let (store, state, saved) = fixture();
        let declaration = declare(&store, &state, &saved);
        assert!(!declaration.args.iter().any(|arg| arg == "--task-id"));
        assert!(!declaration.args.iter().any(|arg| arg == "--attempt"));
    }
}
