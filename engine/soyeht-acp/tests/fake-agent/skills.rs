//! Skills the way Claude Code loads them through claude-agent-acp: every
//! `<additional directory>/.claude/skills/<name>/SKILL.md` is a slash command,
//! and `/<name> arguments` runs it. `write-skill <name>` writes a new skill
//! into the first directory's `.agents/skills`, as an agent asked to create a
//! skill would, then advertises the commands again.

use crate::{Runtime, state::Session};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn directories(params: &Value) -> Vec<String> {
    params["additionalDirectories"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// (name, description, first instruction line) of each readable skill.
fn skills(session: &Session) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for directory in &session.additional_directories {
        let Ok(entries) = fs::read_dir(Path::new(directory).join(".claude/skills")) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(text) = fs::read_to_string(entry.path().join("SKILL.md")) else {
                continue;
            };
            let field = |key: &str| {
                text.lines()
                    .find_map(|line| line.strip_prefix(key))
                    .map(|value| value.trim().trim_matches('"').to_owned())
            };
            let body = text.splitn(3, "---").nth(2).unwrap_or_default();
            let first = body.lines().map(str::trim).find(|line| !line.is_empty());
            if let Some(description) = field("description:") {
                found.push((name, description, first.unwrap_or_default().to_owned()));
            }
        }
    }
    found.sort();
    found
}

pub fn advertise(runtime: &Runtime, session: &Session) {
    let mut commands = vec![json!({"name":"fake-status","description":"Built-in test command"})];
    commands.extend(
        skills(session)
            .into_iter()
            .map(|(name, description, _)| json!({"name":name,"description":description})),
    );
    runtime.update(
        &session.id,
        json!({"sessionUpdate":"available_commands_update","availableCommands":commands}),
    );
}

pub fn turn(runtime: &Runtime, session: &Session, text: &str) -> Option<String> {
    if let Some(name) = text.strip_prefix("write-skill ") {
        let directory = session.additional_directories.first()?;
        let folder = Path::new(directory).join(".agents/skills").join(name);
        fs::create_dir_all(&folder).ok()?;
        let manifest = format!(
            "---\nname: {name}\ndescription: Written by the test agent.\n---\n\nSay {name} done.\n"
        );
        fs::write(folder.join("SKILL.md"), manifest).ok()?;
        advertise(runtime, session);
        return Some(format!("Wrote skill {name}"));
    }
    let command = text.strip_prefix('/')?;
    let (name, arguments) = command.split_once(' ').unwrap_or((command, ""));
    let (_, _, instruction) = skills(session).into_iter().find(|skill| skill.0 == name)?;
    Some(format!(
        "Skill {name}: {instruction} Arguments: {arguments}"
    ))
}
