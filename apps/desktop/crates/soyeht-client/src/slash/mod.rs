//! The composer's "/" list: the Soyeht skills and the commands the pane's harness advertised,
//! what typing filters them to, the text choosing one leaves in the composer, and the prompt
//! that is sent for composer text. These are the Mac's rules (`SlashCatalog`), held together
//! by `contracts/vectors/slash-completion.json`, which both clients pass in full. The window
//! only draws the rows and forwards keys.

mod replica;
#[cfg(test)]
mod tests;

use soyeht_protocol::{AgentCommand, Skill, SkillScope};

/// How many rows the list shows at most.
pub const VISIBLE_ROWS: usize = 6;

/// Where an entry of the list comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Skill(SkillScope),
    Command,
}

/// One entry of the list: a Soyeht skill or a command the harness advertised. Choosing it puts
/// `/name ` in the composer.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub name: String,
    pub detail: String,
    /// What the command takes after its name, when the harness says.
    pub hint: Option<String>,
    pub source: Source,
}

/// The skills of a pane's workspace merged with the pane's harness commands.
///
/// A skill the harness also advertises appears once, as the skill: a delivered skill is listed
/// by its name by one harness and as `$name` by another. Sending keeps the harness's own
/// invocation and falls back to pointing the agent at the skill's `SKILL.md` when the harness
/// does not know the skill.
pub struct Catalog<'a> {
    skills: Vec<&'a Skill>,
    commands: &'a [AgentCommand],
}

impl<'a> Catalog<'a> {
    /// `skills` may hold broken folders and a user skill that a workspace skill of the same
    /// name hides; neither is offered.
    pub fn new(skills: &'a [Skill], commands: &'a [AgentCommand]) -> Self {
        let mut usable: Vec<&Skill> = Vec::new();
        for skill in skills.iter().filter(|skill| skill.problem.is_none()) {
            match usable.iter().position(|known| known.name == skill.name) {
                Some(at) => {
                    if skill.scope == SkillScope::Workspace {
                        usable[at] = skill;
                    }
                }
                None => usable.push(skill),
            }
        }
        Self {
            skills: usable,
            commands,
        }
    }

    /// Every entry: the skills, then the commands that are not a harness copy of a skill.
    pub fn suggestions(&self) -> Vec<Suggestion> {
        let skills = self.skills.iter().map(|skill| Suggestion {
            name: skill.name.clone(),
            detail: skill.description.clone().unwrap_or_default(),
            hint: None,
            source: Source::Skill(skill.scope.clone()),
        });
        let commands = self
            .commands
            .iter()
            .filter(|command| {
                let name = skill_name(&command.name);
                !self.skills.iter().any(|skill| skill.name == name)
            })
            .map(|command| Suggestion {
                name: command.name.clone(),
                detail: command.description.clone(),
                hint: command.input_hint.clone(),
                source: Source::Command,
            });
        skills.chain(commands).collect()
    }

    /// The entries for `query`: names that start with it, then names that contain it, then
    /// descriptions with a word that starts with it. Each group keeps the order of the list.
    pub fn matches(&self, query: &str) -> Vec<Suggestion> {
        let query = query.to_lowercase();
        if query.is_empty() {
            return self.suggestions();
        }
        let mut ranked: Vec<(u8, usize, Suggestion)> = Vec::new();
        for (offset, suggestion) in self.suggestions().into_iter().enumerate() {
            let name = skill_name(&suggestion.name).to_lowercase();
            let rank = if name.starts_with(&query) {
                0
            } else if name.contains(&query) {
                1
            } else if suggestion
                .detail
                .to_lowercase()
                .split(|character: char| !character.is_alphabetic())
                .any(|word| word.starts_with(&query))
            {
                2
            } else {
                continue;
            };
            ranked.push((rank, offset, suggestion));
        }
        ranked.sort_by_key(|(rank, offset, _)| (*rank, *offset));
        ranked
            .into_iter()
            .map(|(_, _, suggestion)| suggestion)
            .collect()
    }

    /// The prompt to send for composer text. Only `/skill arguments` for a skill the harness
    /// does not advertise under its own name changes.
    pub fn prompt(&self, text: &str) -> String {
        let Some(rest) = text.strip_prefix('/') else {
            return text.to_owned();
        };
        let rest = rest.trim_start();
        // The arguments are what follows the one space after the name, as they were typed.
        let (name, arguments) = match rest.find(char::is_whitespace) {
            Some(end) => {
                let tail = &rest[end..];
                let skip = tail.chars().next().map_or(0, char::len_utf8);
                (&rest[..end], &tail[skip..])
            }
            None => (rest, ""),
        };
        if name.is_empty() || self.commands.iter().any(|command| command.name == name) {
            return text.to_owned();
        }
        let Some(skill) = self.skills.iter().find(|skill| skill.name == name) else {
            return text.to_owned();
        };
        let harness_copy = self
            .commands
            .iter()
            .any(|command| command.name.strip_prefix('$') == Some(name));
        if harness_copy {
            return if arguments.is_empty() {
                format!("${name}")
            } else {
                format!("${name} {arguments}")
            };
        }
        let request = format!(
            "Use the Soyeht skill \"{name}\": read {} and follow it.",
            manifest(&skill.path)
        );
        if arguments.is_empty() {
            request
        } else {
            format!("{request}\n\n{arguments}")
        }
    }
}

/// The word typed after a leading "/", while the composer holds nothing else.
pub fn query(text: &str) -> Option<&str> {
    let query = text.strip_prefix('/')?;
    (!query.contains(char::is_whitespace)).then_some(query)
}

/// What choosing `suggestion` leaves in the composer: `/name ` with the caret after the space,
/// so typing goes on with the arguments, not inside the name.
pub fn accept(suggestion: &Suggestion) -> String {
    format!("/{} ", suggestion.name)
}

/// A skill listed by a harness that writes it `$name`.
fn skill_name(command: &str) -> &str {
    command.strip_prefix('$').unwrap_or(command)
}

/// The `SKILL.md` of the skill folder at `path`, written with the separator the path uses.
fn manifest(path: &str) -> String {
    let separator = if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    };
    format!("{}{separator}SKILL.md", path.trim_end_matches(['/', '\\']))
}

/// What the person has done with the list since it opened: which row is chosen and which query
/// they closed it for. The window keeps one per composer.
#[derive(Debug, Default)]
pub struct List {
    selection: usize,
    dismissed: Option<String>,
    /// The text the rows were last made for: a change of text chooses the first row again.
    seen: String,
}

impl List {
    /// The rows to show for the composer's `text`: none unless it is `/` and one word, or the
    /// person closed the list for this very query.
    pub fn rows(&mut self, catalog: &Catalog, text: &str) -> Vec<Suggestion> {
        if self.seen != text {
            self.seen = text.to_owned();
            self.selection = 0;
        }
        let query = query(text);
        if query != self.dismissed.as_deref() {
            self.dismissed = None;
        }
        match query {
            Some(query) if self.dismissed.is_none() => catalog
                .matches(query)
                .into_iter()
                .take(VISIBLE_ROWS)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Moves the choice `by` rows, past the last row to the first and back.
    pub fn step(&mut self, by: isize, rows: usize) {
        if rows == 0 {
            return;
        }
        let rows = rows as isize;
        let here = self.selection.min(rows as usize - 1) as isize;
        self.selection = (here + by).rem_euclid(rows) as usize;
    }

    /// The row Return, Tab or a click would put in the composer.
    pub fn selected(&self, rows: &[Suggestion]) -> Option<usize> {
        (!rows.is_empty()).then(|| self.selection.min(rows.len() - 1))
    }

    /// Escape: closes the list until the query changes.
    pub fn dismiss(&mut self, text: &str) {
        self.dismissed = query(text).map(str::to_owned);
    }
}
