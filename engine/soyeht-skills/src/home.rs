//! Where Soyeht skills live on disk, and the folder shapes harnesses read.
//!
//! The user's skills home `<data>/skills` is laid out like a project root:
//! the skills sit in `.agents/skills`, the cross-tool convention Codex and
//! others read, and `.claude/skills` links to the same folder for Claude Code.
//! A session that lists the home as an ACP additional directory therefore
//! loads the skills natively in both harnesses, without anything written to
//! `~/.claude` or `~/.codex`.
//!
//! A workspace keeps its own skills in `<workspace>/.agents/skills`. Codex
//! reads that folder from the project itself; Claude Code reads only
//! `.claude/skills`, so the engine gives it a bridge folder whose
//! `.claude/skills` links to the workspace's skills.

use crate::{MANIFEST, render};
use sha2::{Digest, Sha256};
use soyeht_platform::fs::replace_dir_link;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const HOME: &str = "skills";
const BRIDGES: &str = "skill-bridges";
const AGENTS_SKILLS: [&str; 2] = [".agents", "skills"];
const CLAUDE_SKILLS: [&str; 2] = [".claude", "skills"];
/// The starter skill that teaches any agent to write new skills.
pub const GUIDE: &str = "create-skill";

/// The folder an ACP session lists as an additional directory.
pub fn user_home(data_directory: &Path) -> PathBuf {
    data_directory.join(HOME)
}

/// The folder holding the user's skill folders.
pub fn user_store(data_directory: &Path) -> PathBuf {
    agents_skills(&user_home(data_directory))
}

/// The folder holding a workspace's skill folders.
pub fn workspace_store(root: &Path) -> PathBuf {
    agents_skills(root)
}

fn agents_skills(root: &Path) -> PathBuf {
    AGENTS_SKILLS
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn claude_skills(root: &Path) -> PathBuf {
    CLAUDE_SKILLS
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Creates the user's skills home and its Claude Code link. The first time
/// the store is created it also gets the starter guide; a person who deletes
/// the guide later keeps it deleted.
pub fn prepare_user_home(data_directory: &Path) -> io::Result<()> {
    let home = user_home(data_directory);
    let store = user_store(data_directory);
    let created = !store.exists();
    fs::create_dir_all(&store)?;
    replace_dir_link(&claude_skills(&home), Path::new("../.agents/skills"))?;
    if created {
        let guide = store.join(GUIDE);
        fs::create_dir_all(&guide)?;
        fs::write(guide.join(MANIFEST), guide_text(&store))?;
    }
    Ok(())
}

/// A folder whose `.claude/skills` links to the skills of the project at
/// `cwd`, or `None` when that project keeps no skills.
pub fn workspace_bridge(data_directory: &Path, cwd: &Path) -> io::Result<Option<PathBuf>> {
    let store = workspace_store(cwd);
    if !store.is_dir() {
        return Ok(None);
    }
    let digest = format!("{:x}", Sha256::digest(cwd.as_os_str().as_encoded_bytes()));
    let bridge = data_directory.join(BRIDGES).join(&digest[..16]);
    let target = claude_skills(&bridge);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    replace_dir_link(&target, &store)?;
    Ok(Some(bridge))
}

fn guide_text(store: &Path) -> String {
    let store = store.display();
    let description = "Create or improve a Soyeht skill, a reusable SKILL.md every agent in \
        Soyeht can load. Use when the user asks to create, save, teach or fix a skill.";
    let instructions = format!(
        "# Create a Soyeht skill

Soyeht skills are folders in {store}. Every harness in Soyeht (Claude Code, Codex and \
others) picks up a new or changed skill without a restart.

1. Choose a name: lowercase letters, digits and single hyphens, at most 64 characters. \
Create the folder {store}/<name>. A skill for one project goes in that project's \
.agents/skills folder instead.
2. Write <name>/SKILL.md:

   ```
   ---
   name: <name>
   description: <what it does and when to use it, one or two sentences>
   ---

   <short, concrete steps another agent can follow without this conversation>
   ```

   Put long references or scripts next to SKILL.md and link them from it.
3. Test it with a zero-context agent. Start a fresh subagent with no conversation \
history (your harness's subagent feature; if it has none, open a new Soyeht pane with \
the soyeht MCP tools and message it). Give it only a realistic task that should trigger \
the skill. Do not say what the skill is for or how it works. Let it find and load the \
skill and do the task.
4. Compare the result with what the skill is meant to do. If the agent misread it or \
failed, fix SKILL.md and test again.
5. Tell the user what you tested and the outcome."
    );
    render(GUIDE, description, &instructions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!("soyeht-skills-home-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn the_user_home_serves_both_harness_folders_and_seeds_the_guide_once() {
        let data = directory();
        prepare_user_home(&data).unwrap();
        let store = Store::new(user_store(&data));
        let guide = store.read(GUIDE).unwrap().0;
        assert!(guide.manifest.is_ok());
        let via_claude = claude_skills(&user_home(&data)).join(GUIDE).join(MANIFEST);
        assert!(via_claude.is_file());
        store.delete(GUIDE).unwrap();
        prepare_user_home(&data).unwrap();
        assert!(store.list().unwrap().is_empty());
        fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn a_workspace_bridge_exists_only_for_a_project_with_skills() {
        let data = directory();
        let project = directory();
        fs::create_dir_all(&project).unwrap();
        assert_eq!(workspace_bridge(&data, &project).unwrap(), None);
        Store::new(workspace_store(&project))
            .create("deploy", "Deploys.", "")
            .unwrap();
        let bridge = workspace_bridge(&data, &project).unwrap().unwrap();
        assert_eq!(
            workspace_bridge(&data, &project).unwrap(),
            Some(bridge.clone())
        );
        assert!(
            claude_skills(&bridge)
                .join("deploy")
                .join(MANIFEST)
                .is_file()
        );
        fs::remove_dir_all(&data).unwrap();
        fs::remove_dir_all(&project).unwrap();
    }
}
