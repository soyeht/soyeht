//! Whether the person trusts a project folder (ADR 0150, contract 18).
//!
//! One row per canonical folder holds the answer the person gave: trusted, or kept restricted. A
//! folder with no row is undecided and is treated as restricted. The state is the single answer to
//! "may this folder's own files run code?", read by the language servers and, later, by the
//! repository-defined helpers. Only a decision of the person writes it; nothing here derives one.
use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, params};
use soyeht_protocol::{ProjectTrustChoice, limits::EDITOR_PATH_BYTES};

fn valid(root: &str) -> bool {
    !root.is_empty() && root.len() <= EDITOR_PATH_BYTES
}

fn state_text(choice: &ProjectTrustChoice) -> &'static str {
    match choice {
        ProjectTrustChoice::Restricted => "restricted",
        ProjectTrustChoice::Trusted => "trusted",
    }
}

fn parse(text: &str) -> Result<ProjectTrustChoice> {
    match text {
        "restricted" => Ok(ProjectTrustChoice::Restricted),
        "trusted" => Ok(ProjectTrustChoice::Trusted),
        _ => Err(Error::InvalidProjectTrust),
    }
}

/// The answer given for the folder `root`, if any.
pub fn decided(store: &Store, root: &str) -> Result<Option<ProjectTrustChoice>> {
    store
        .connection()
        .query_row(
            "SELECT state FROM project_trust WHERE root=?1",
            [root],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|text| parse(&text))
        .transpose()
}

/// Every folder with an answer, in folder order.
pub fn all(store: &Store) -> Result<Vec<(String, ProjectTrustChoice)>> {
    let mut statement = store
        .connection()
        .prepare("SELECT root,state FROM project_trust ORDER BY root")?;
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|row| {
            let (root, text) = row?;
            Ok((root, parse(&text)?))
        })
        .collect()
}

/// Keeps `choice` as the answer for the folder `root`, replacing an earlier one.
pub fn decide(change: &Change<'_>, root: &str, choice: &ProjectTrustChoice) -> Result<()> {
    if !valid(root) {
        return Err(Error::InvalidProjectTrust);
    }
    change.connection().execute(
        "INSERT INTO project_trust(root,state) VALUES (?1,?2) ON CONFLICT(root) DO UPDATE SET state=excluded.state,decided_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        params![root, state_text(choice)],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_round_trips_through_its_text() {
        for choice in [ProjectTrustChoice::Restricted, ProjectTrustChoice::Trusted] {
            assert_eq!(parse(state_text(&choice)).unwrap(), choice);
        }
        assert!(matches!(
            parse("undecided"),
            Err(Error::InvalidProjectTrust)
        ));
    }

    #[test]
    fn an_empty_or_oversized_folder_is_not_a_key() {
        assert!(!valid(""));
        assert!(!valid(&"a".repeat(EDITOR_PATH_BYTES + 1)));
        assert!(valid("/work/project"));
    }
}
