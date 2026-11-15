//! The avatar library and the binding of an avatar to a workspace (ADR 0166, contract 19).
//!
//! An avatar is global to the person: a name, a source picture (kept by its hash; the file is in
//! the avatar's folder), a build state and a default voice. A binding is a workspace's own: the
//! harness the avatar thinks with, the speech settings, the persona and the clients that have voice
//! on. The store keeps identifiers and settings only. A key of a speech or image provider is never a
//! column here, and a persona is never part of an error.
use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};
use soyeht_protocol::{
    Avatar, AvatarBinding, AvatarBuildState, AvatarHearing, AvatarKind, AvatarVoice,
};

const AVATAR_COLUMNS: &str = "id,name,kind,source_hash,build_state,failure_kind,package_hash,voice_provider,voice_id,created_at";
const BINDING_COLUMNS: &str = "workspace_id,avatar_id,harness,model,effort,hearing_provider,hearing_language,voice_provider,voice_id,persona";

/// The text a kind or a state is stored as: its name on the wire.
fn text_of<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(text) => Ok(text),
        _ => Err(Error::InvalidAvatar),
    }
}

/// The value a stored text names. A text this build does not know is an invalid row, never a guess.
fn value_of<T: DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|_| Error::InvalidAvatar)
}

fn voice_of(provider: Option<String>, voice: Option<String>) -> Option<AvatarVoice> {
    Some(AvatarVoice {
        provider_id: provider?,
        voice_id: voice?,
    })
}

/// The columns of an avatar row, as the text they were written as.
struct AvatarRow {
    id: String,
    name: String,
    kind: String,
    source_hash: String,
    state: String,
    failure: Option<String>,
    package_hash: Option<String>,
    voice: Option<AvatarVoice>,
    created_at: String,
}

fn avatar_row(row: &Row<'_>) -> rusqlite::Result<AvatarRow> {
    Ok(AvatarRow {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        source_hash: row.get(3)?,
        state: row.get(4)?,
        failure: row.get(5)?,
        package_hash: row.get(6)?,
        voice: voice_of(row.get(7)?, row.get(8)?),
        created_at: row.get(9)?,
    })
}

fn avatar_of(row: AvatarRow) -> Result<Avatar> {
    Ok(Avatar {
        avatar_id: row.id,
        name: row.name,
        kind: value_of(&row.kind)?,
        source_hash: row.source_hash,
        build_state: value_of(&row.state)?,
        failure_kind: row.failure.as_deref().map(value_of).transpose()?,
        package_hash: row.package_hash,
        voice: row.voice,
        created_at: row.created_at,
    })
}

/// The columns of a binding row, before the clients with voice on are read.
struct BindingRow {
    workspace_id: String,
    avatar_id: String,
    harness: String,
    model: Option<String>,
    effort: Option<String>,
    hearing: Option<AvatarHearing>,
    voice: Option<AvatarVoice>,
    persona: String,
}

fn binding_row(row: &Row<'_>) -> rusqlite::Result<BindingRow> {
    let hearing_provider: Option<String> = row.get(5)?;
    let hearing_language: Option<String> = row.get(6)?;
    Ok(BindingRow {
        workspace_id: row.get(0)?,
        avatar_id: row.get(1)?,
        harness: row.get(2)?,
        model: row.get(3)?,
        effort: row.get(4)?,
        hearing: hearing_provider.map(|provider_id| AvatarHearing {
            provider_id,
            language: hearing_language,
        }),
        voice: voice_of(row.get(7)?, row.get(8)?),
        persona: row.get(9)?,
    })
}

fn voice_on(connection: &rusqlite::Connection, workspace_id: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT viewer FROM workspace_avatar_voice WHERE workspace_id=?1 ORDER BY viewer",
    )?;
    let viewers = statement
        .query_map([workspace_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(viewers)
}

fn binding_of(connection: &rusqlite::Connection, row: BindingRow) -> Result<AvatarBinding> {
    let voice_on = voice_on(connection, &row.workspace_id)?;
    Ok(AvatarBinding {
        workspace_id: row.workspace_id,
        avatar_id: row.avatar_id,
        harness: row.harness,
        model: row.model,
        effort: row.effort,
        hearing: row.hearing,
        voice: row.voice,
        persona: row.persona,
        voice_on,
    })
}

/// Every avatar of the library, oldest first.
pub fn list(store: &Store) -> Result<Vec<Avatar>> {
    let mut statement = store.connection().prepare(&format!(
        "SELECT {AVATAR_COLUMNS} FROM avatars ORDER BY created_seq"
    ))?;
    let rows = statement
        .query_map([], avatar_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(avatar_of).collect()
}

/// The avatar `avatar_id`, if the library has it.
pub fn get(store: &Store, avatar_id: &str) -> Result<Option<Avatar>> {
    store
        .connection()
        .query_row(
            &format!("SELECT {AVATAR_COLUMNS} FROM avatars WHERE id=?1"),
            [avatar_id],
            avatar_row,
        )
        .optional()?
        .map(avatar_of)
        .transpose()
}

/// How many avatars the library holds.
pub fn count(store: &Store) -> Result<usize> {
    let count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM avatars", [], |row| row.get(0))?;
    usize::try_from(count).map_err(|_| Error::InvalidAvatar)
}

/// Adds `avatar`, which starts with nothing built: its state is `none`, whatever it says.
pub fn insert_in(change: &Change<'_>, avatar: &Avatar) -> Result<()> {
    if avatar.avatar_id.is_empty() || avatar.name.is_empty() || avatar.source_hash.len() != 64 {
        return Err(Error::InvalidAvatar);
    }
    change.connection().execute(
        "INSERT INTO avatars(id,name,kind,source_hash,build_state,voice_provider,voice_id,created_at,created_seq) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,(SELECT COALESCE(MAX(created_seq),0)+1 FROM avatars))",
        params![
            avatar.avatar_id,
            avatar.name,
            text_of::<AvatarKind>(&avatar.kind)?,
            avatar.source_hash,
            text_of(&AvatarBuildState::None)?,
            avatar.voice.as_ref().map(|voice| voice.provider_id.as_str()),
            avatar.voice.as_ref().map(|voice| voice.voice_id.as_str()),
            avatar.created_at,
        ],
    )?;
    Ok(())
}

/// Renames the avatar. False when the library has none with that id.
pub fn rename_in(change: &Change<'_>, avatar_id: &str, name: &str) -> Result<bool> {
    if name.is_empty() {
        return Err(Error::InvalidAvatar);
    }
    let renamed = change.connection().execute(
        "UPDATE avatars SET name=?1 WHERE id=?2",
        params![name, avatar_id],
    )?;
    Ok(renamed == 1)
}

/// Starts a build: an avatar with nothing built, or whose last build failed, becomes `building`
/// and forgets the failure. False when it was in any other state, which is left as it was.
pub fn start_build_in(change: &Change<'_>, avatar_id: &str) -> Result<bool> {
    let started = change.connection().execute(
        "UPDATE avatars SET build_state=?1, failure_kind=NULL, package_hash=NULL WHERE id=?2 AND build_state IN (?3,?4)",
        params![
            text_of(&AvatarBuildState::Building)?,
            avatar_id,
            text_of(&AvatarBuildState::None)?,
            text_of(&AvatarBuildState::Failed)?,
        ],
    )?;
    Ok(started == 1)
}

/// Removes the avatar and the bindings of closed workspaces that name it. A binding of an open
/// workspace is the caller's to refuse beforehand ([`open_workspaces_of`]): the foreign key fails
/// the change if one is left.
pub fn delete_in(change: &Change<'_>, avatar_id: &str) -> Result<bool> {
    let connection = change.connection();
    connection.execute(
        "DELETE FROM workspace_avatars WHERE avatar_id=?1",
        [avatar_id],
    )?;
    let removed = connection.execute("DELETE FROM avatars WHERE id=?1", [avatar_id])?;
    Ok(removed == 1)
}

/// The open workspaces that have the avatar bound, in the order they were made.
pub fn open_workspaces_of(store: &Store, avatar_id: &str) -> Result<Vec<String>> {
    let mut statement = store.connection().prepare(
        "SELECT workspace_avatars.workspace_id FROM workspace_avatars JOIN workspaces ON workspaces.id = workspace_avatars.workspace_id WHERE workspace_avatars.avatar_id=?1 AND workspaces.closed=0 ORDER BY workspaces.created_seq",
    )?;
    let ids = statement
        .query_map([avatar_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// The binding of every open workspace that has an avatar, in the order the workspaces were made.
pub fn bindings(store: &Store) -> Result<Vec<AvatarBinding>> {
    let mut statement = store.connection().prepare(&format!(
        "SELECT {BINDING_COLUMNS} FROM workspace_avatars JOIN workspaces ON workspaces.id = workspace_avatars.workspace_id WHERE workspaces.closed=0 ORDER BY workspaces.created_seq"
    ))?;
    let rows = statement
        .query_map([], binding_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|row| binding_of(store.connection(), row))
        .collect()
}

/// The binding of one workspace.
pub fn binding(store: &Store, workspace_id: &str) -> Result<Option<AvatarBinding>> {
    store
        .connection()
        .query_row(
            &format!("SELECT {BINDING_COLUMNS} FROM workspace_avatars WHERE workspace_id=?1"),
            [workspace_id],
            binding_row,
        )
        .optional()?
        .map(|row| binding_of(store.connection(), row))
        .transpose()
}

/// Binds `binding.avatar_id` to the workspace, replacing the binding it had. The clients with voice
/// on are kept: they are changed by [`set_voice_in`], never by a rebinding.
pub fn bind_in(change: &Change<'_>, binding: &AvatarBinding) -> Result<()> {
    if binding.harness.is_empty() {
        return Err(Error::InvalidAvatar);
    }
    change.connection().execute(
        &format!(
            "INSERT INTO workspace_avatars({BINDING_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(workspace_id) DO UPDATE SET avatar_id=excluded.avatar_id, harness=excluded.harness, model=excluded.model, effort=excluded.effort, hearing_provider=excluded.hearing_provider, hearing_language=excluded.hearing_language, voice_provider=excluded.voice_provider, voice_id=excluded.voice_id, persona=excluded.persona"
        ),
        params![
            binding.workspace_id,
            binding.avatar_id,
            binding.harness,
            binding.model,
            binding.effort,
            binding.hearing.as_ref().map(|hearing| hearing.provider_id.as_str()),
            binding.hearing.as_ref().and_then(|hearing| hearing.language.as_deref()),
            binding.voice.as_ref().map(|voice| voice.provider_id.as_str()),
            binding.voice.as_ref().map(|voice| voice.voice_id.as_str()),
            binding.persona,
        ],
    )?;
    Ok(())
}

/// Turns voice on or off for one client of a bound workspace. Nothing happens for a workspace
/// that has no avatar.
pub fn set_voice_in(change: &Change<'_>, workspace_id: &str, viewer: &str, on: bool) -> Result<()> {
    let connection = change.connection();
    if on {
        connection.execute(
            "INSERT OR IGNORE INTO workspace_avatar_voice(workspace_id,viewer) SELECT workspace_id, ?2 FROM workspace_avatars WHERE workspace_id=?1",
            params![workspace_id, viewer],
        )?;
    } else {
        connection.execute(
            "DELETE FROM workspace_avatar_voice WHERE workspace_id=?1 AND viewer=?2",
            params![workspace_id, viewer],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_and_a_state_are_stored_as_their_names_on_the_wire() {
        assert_eq!(text_of(&AvatarKind::Uploaded).unwrap(), "uploaded");
        assert_eq!(text_of(&AvatarBuildState::Building).unwrap(), "building");
        assert_eq!(
            value_of::<AvatarBuildState>("failed").unwrap(),
            AvatarBuildState::Failed
        );
    }

    #[test]
    fn a_text_nobody_defined_is_an_invalid_row() {
        assert!(matches!(
            value_of::<AvatarKind>("hologram"),
            Err(Error::InvalidAvatar)
        ));
    }
}
