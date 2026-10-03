//! The skill folders an agent session loads natively. An agent that
//! advertises ACP `sessionCapabilities.additionalDirectories` gets the user's
//! skills home and, when the project keeps skills, its Claude Code bridge.
//! Claude Code then reads their `.claude/skills` and watches them; codex-acp
//! registers their `.agents/skills` as extra skill roots.

use serde_json::{Value, json};
use soyeht_skills::home;
use std::path::{Path, PathBuf};

pub(crate) fn session_directories(
    data_directory: &Path,
    accepts: bool,
    mut params: Value,
) -> Value {
    let Some(cwd) = params["cwd"]
        .as_str()
        .map(PathBuf::from)
        .filter(|_| accepts)
    else {
        return params;
    };
    // A folder that cannot be prepared is left out; the session still opens.
    let mut directories = Vec::new();
    if home::prepare_user_home(data_directory).is_ok() {
        directories.push(home::user_home(data_directory));
    }
    if let Ok(Some(bridge)) = home::workspace_bridge(data_directory, &cwd) {
        directories.push(bridge);
    }
    let directories: Vec<&str> = directories
        .iter()
        .filter_map(|path| path.to_str())
        .collect();
    if !directories.is_empty() {
        params["additionalDirectories"] = json!(directories);
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_skills::Store;
    use std::fs;

    #[test]
    fn only_agents_that_accept_directories_receive_the_skill_folders() {
        let data = std::env::temp_dir().join(format!("soyeht-delivery-{}", uuid::Uuid::new_v4()));
        let project = data.join("project");
        fs::create_dir_all(&project).unwrap();
        let params = json!({"cwd":project,"mcpServers":[]});
        assert_eq!(session_directories(&data, false, params.clone()), params);
        let user_only = session_directories(&data, true, params.clone());
        assert_eq!(
            user_only["additionalDirectories"],
            json!([home::user_home(&data)])
        );
        Store::new(home::workspace_store(&project))
            .create("deploy", "Deploys.", "")
            .unwrap();
        let both = session_directories(&data, true, params);
        let directories = both["additionalDirectories"].as_array().unwrap();
        assert_eq!(directories.len(), 2);
        let bridged = Path::new(directories[1].as_str().unwrap()).join(".claude/skills/deploy");
        assert!(bridged.join(soyeht_skills::MANIFEST).is_file());
        fs::remove_dir_all(&data).unwrap();
    }
}
