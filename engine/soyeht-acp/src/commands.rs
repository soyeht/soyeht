//! ACP `available_commands_update`: the slash commands an agent offers.
//! Each update replaces the previous list (Claude Code re-sends it when it
//! discovers skills mid-session). Entries a client could not show are
//! dropped rather than failing the update.

use crate::Error;
use serde_json::Value;
use soyeht_protocol::{AgentCommand, limits};

const NAME_CHARS: usize = 128;
const DESCRIPTION_CHARS: usize = 1024;
const HINT_CHARS: usize = 256;

pub fn parse(update: &Value) -> Result<Vec<AgentCommand>, Error> {
    let commands = update["availableCommands"]
        .as_array()
        .ok_or(Error::MissingCommands)?;
    let mut parsed: Vec<AgentCommand> = Vec::new();
    for command in commands.iter().filter_map(command) {
        if parsed.len() == limits::AGENT_COMMANDS {
            break;
        }
        if !parsed.iter().any(|known| known.name == command.name) {
            parsed.push(command);
        }
    }
    Ok(parsed)
}

fn command(value: &Value) -> Option<AgentCommand> {
    let name = value["name"].as_str().filter(|name| {
        !name.is_empty()
            && name.chars().count() <= NAME_CHARS
            && !name.chars().any(|c| c.is_whitespace() || c.is_control())
    })?;
    Some(AgentCommand {
        name: name.into(),
        description: clip(
            value["description"].as_str().unwrap_or_default(),
            DESCRIPTION_CHARS,
        ),
        input_hint: value["input"]["hint"]
            .as_str()
            .map(|hint| clip(hint, HINT_CHARS))
            .filter(|hint| !hint.is_empty()),
    })
}

fn clip(text: &str, characters: usize) -> String {
    text.trim().chars().take(characters).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn commands_keep_names_hints_and_drop_what_cannot_be_typed() {
        let update = json!({"availableCommands":[
            {"name":"review","description":"Reviews a change.","input":{"hint":"branch"}},
            {"name":"$deploy","description":"Deploys.","input":null},
            {"name":"review","description":"Duplicate."},
            {"name":"two words","description":"Unreachable."},
            {"name":"","description":"Empty."},
            {"description":"Nameless."}
        ]});
        let commands = parse(&update).unwrap();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].input_hint.as_deref(), Some("branch"));
        assert_eq!(commands[1].name, "$deploy");
        assert_eq!(commands[1].input_hint, None);
        let many: Vec<_> = (0..limits::AGENT_COMMANDS + 5)
            .map(|index| json!({"name":format!("c{index}"),"description":"d".repeat(2000)}))
            .collect();
        let bounded = parse(&json!({"availableCommands":many})).unwrap();
        assert_eq!(bounded.len(), limits::AGENT_COMMANDS);
        assert_eq!(bounded[0].description.chars().count(), DESCRIPTION_CHARS);
        assert!(parse(&json!({})).is_err());
    }
}
