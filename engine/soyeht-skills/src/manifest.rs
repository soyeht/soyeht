//! The `SKILL.md` format of the open Agent Skills specification
//! (<https://agentskills.io/specification>): YAML frontmatter with a `name`
//! and a `description`, then Markdown instructions.
//!
//! Soyeht reads `name`, `description` and, to show it, `allowed-tools`. Every
//! other field (Claude Code's `disable-model-invocation`, the standard's
//! `license` or `metadata`, ...) stays in the file untouched, because each
//! harness reads its own fields from the same file.

use crate::Problem;

pub(crate) const NAME_CHARS: usize = 64;
pub(crate) const DESCRIPTION_CHARS: usize = 1024;

/// The fields Soyeht reads from a valid `SKILL.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    /// Tools a harness may run without asking while the skill is active,
    /// as written (Claude Code's `allowed-tools`). Shown, never enforced.
    pub allowed_tools: Option<String>,
}

/// A name the specification accepts: 1 to 64 lowercase letters, digits and
/// single hyphens, neither first nor last.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= NAME_CHARS
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

/// Reads the frontmatter of `text`, the content of `<directory>/SKILL.md`.
pub fn parse(directory: &str, text: &str) -> Result<Manifest, Problem> {
    let fields = frontmatter(text)?;
    let field = |key: &str| {
        fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let name = field("name").ok_or(Problem::MissingName)?;
    if !valid_name(&name) {
        return Err(Problem::InvalidName);
    }
    if name != directory {
        return Err(Problem::NameMismatch);
    }
    let description = field("description").ok_or(Problem::MissingDescription)?;
    if description.chars().count() > DESCRIPTION_CHARS {
        return Err(Problem::DescriptionTooLong);
    }
    Ok(Manifest {
        name,
        description,
        allowed_tools: field("allowed-tools"),
    })
}

/// A new `SKILL.md`. The description is written as a YAML double-quoted
/// scalar, so any text round-trips through `parse`.
pub fn render(name: &str, description: &str, instructions: &str) -> String {
    let quoted: String = description
        .chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            '\t' => vec!['\\', 't'],
            c => vec![c],
        })
        .collect();
    let body = instructions.trim();
    format!("---\nname: {name}\ndescription: \"{quoted}\"\n---\n\n{body}\n")
}

/// Top-level `key: value` pairs between the opening and closing `---`.
fn frontmatter(text: &str) -> Result<Vec<(String, String)>, Problem> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(Problem::MissingFrontmatter);
    }
    let mut block = Vec::new();
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            return Ok(fields(&block));
        }
        block.push(line);
    }
    Err(Problem::UnterminatedFrontmatter)
}

fn fields(lines: &[&str]) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        let Some((key, value)) = top_level_entry(line) else {
            continue;
        };
        let start = index;
        while index < lines.len() && continues(lines[index]) {
            index += 1;
        }
        fields.push((key.to_owned(), scalar(value.trim(), &lines[start..index])));
    }
    fields
}

fn top_level_entry(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let simple = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (simple && (value.is_empty() || value.starts_with(' '))).then_some((key, value))
}

/// A line that belongs to the previous key: indented, or blank.
fn continues(line: &str) -> bool {
    line.trim().is_empty() || line.starts_with([' ', '\t'])
}

fn scalar(first: &str, rest: &[&str]) -> String {
    let rest = rest.iter().map(|line| line.trim());
    if let Some(indicator) = first.chars().next().filter(|c| matches!(c, '|' | '>')) {
        let lines: Vec<&str> = rest.collect();
        let separator = if indicator == '|' { "\n" } else { " " };
        return lines.join(separator).trim().to_owned();
    }
    let joined = std::iter::once(first)
        .chain(rest)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    unquote(&joined)
}

fn unquote(value: &str) -> String {
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_owned();
    };
    let mut output = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(c) = characters.next() {
        if c != '\\' {
            output.push(c);
            continue;
        }
        match characters.next() {
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_specification() {
        for name in ["pdf", "code-review", "a1-b2"] {
            assert!(valid_name(name), "{name}");
        }
        let long = "a".repeat(NAME_CHARS + 1);
        for name in [
            "",
            "-pdf",
            "pdf-",
            "pd--f",
            "PDF",
            "pdf tools",
            "pdf_x",
            &long,
        ] {
            assert!(!valid_name(name), "{name}");
        }
    }

    #[test]
    fn frontmatter_scalars_quotes_and_blocks_are_read() {
        let text = "---\nname: review\nallowed-tools: Read, Grep\ndescription: >\n  Reviews a change.\n  Use for pull requests.\nmetadata:\n  author: me\n---\n# Body\n";
        let manifest = parse("review", text).unwrap();
        assert_eq!(manifest.name, "review");
        assert_eq!(manifest.allowed_tools.as_deref(), Some("Read, Grep"));
        assert_eq!(
            manifest.description,
            "Reviews a change. Use for pull requests."
        );
        let quoted = "---\nname: 'it'\ndescription: \"Say \\\"hi\\\"\"\n---\n";
        assert_eq!(parse("it", quoted).unwrap().description, "Say \"hi\"");
    }

    #[test]
    fn rendered_manifests_round_trip() {
        let text = render("notes", "Line one\n\"quoted\" \\ path: x", "Do it.");
        let manifest = parse("notes", &text).unwrap();
        assert_eq!(manifest.description, "Line one\n\"quoted\" \\ path: x");
        assert!(text.ends_with("Do it.\n"));
    }

    #[test]
    fn each_defect_has_its_own_problem() {
        let cases = [
            ("# no frontmatter", Problem::MissingFrontmatter),
            ("---\nname: x\n", Problem::UnterminatedFrontmatter),
            ("---\ndescription: d\n---\n", Problem::MissingName),
            (
                "---\nname: Bad Name\ndescription: d\n---\n",
                Problem::InvalidName,
            ),
            (
                "---\nname: other\ndescription: d\n---\n",
                Problem::NameMismatch,
            ),
            ("---\nname: x\n---\n", Problem::MissingDescription),
        ];
        for (text, problem) in cases {
            assert_eq!(parse("x", text), Err(problem), "{text}");
        }
        let long = format!(
            "---\nname: x\ndescription: {}\n---\n",
            "d".repeat(DESCRIPTION_CHARS + 1)
        );
        assert_eq!(parse("x", &long), Err(Problem::DescriptionTooLong));
    }
}
