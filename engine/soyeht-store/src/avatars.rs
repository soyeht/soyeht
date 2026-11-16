//! The avatar library and the binding of an avatar to a workspace (ADR 0166, contract 19).
//!
//! An avatar is global to the person: a name, a source picture (kept by its hash; the file is in
//! the avatar's folder), the catalogue ids of the image provider and model, and a default voice. A
//! binding is a workspace's own: the harness the avatar thinks with, the speech settings and the
//! persona, under a revision that a writer must name. The store keeps identifiers and settings only.
//! A key of a speech or image provider is never a column here, and a persona is never part of an
//! error. Whether a client has voice on is that client's own preference and is not kept.
use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, Row, params};
use soyeht_protocol::{Avatar, AvatarBinding, AvatarHearing, AvatarVoice};

const AVATAR_COLUMNS: &str =
    "id,name,kind,source_hash,image_provider,image_model,voice_provider,voice_id,created_at";
const BINDING_COLUMNS: &str = "workspace_id,avatar_id,harness,model,effort,hearing_provider,hearing_language,voice_provider,voice_id,persona,revision";

fn voice_of(provider: Option<String>, voice: Option<String>) -> Option<AvatarVoice> {
    Some(AvatarVoice {
        provider_id: provider?,
        voice_id: voice?,
    })
}

fn avatar_row(row: &Row<'_>) -> rusqlite::Result<Avatar> {
    Ok(Avatar {
        avatar_id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        source_hash: row.get(3)?,
        image_provider: row.get(4)?,
        image_model: row.get(5)?,
        voice: voice_of(row.get(6)?, row.get(7)?),
        created_at: row.get(8)?,
    })
}

fn binding_row(row: &Row<'_>) -> rusqlite::Result<AvatarBinding> {
    let hearing_provider: Option<String> = row.get(5)?;
    let hearing_language: Option<String> = row.get(6)?;
    let revision: i64 = row.get(10)?;
    Ok(AvatarBinding {
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
        revision: revision.to_string(),
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
    Ok(rows)
}

/// The avatar `avatar_id`, if the library has it.
pub fn get(store: &Store, avatar_id: &str) -> Result<Option<Avatar>> {
    Ok(store
        .connection()
        .query_row(
            &format!("SELECT {AVATAR_COLUMNS} FROM avatars WHERE id=?1"),
            [avatar_id],
            avatar_row,
        )
        .optional()?)
}

/// How many avatars the library holds.
pub fn count(store: &Store) -> Result<usize> {
    let count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM avatars", [], |row| row.get(0))?;
    usize::try_from(count).map_err(|_| Error::InvalidAvatar)
}

/// The identifiers of the avatars, which the folders of `<data>/avatars/` are named by.
pub fn ids(store: &Store) -> Result<Vec<String>> {
    Ok(list(store)?
        .into_iter()
        .map(|avatar| avatar.avatar_id)
        .collect())
}

/// Adds `avatar`.
pub fn insert_in(change: &Change<'_>, avatar: &Avatar) -> Result<()> {
    if avatar.avatar_id.is_empty() || avatar.name.is_empty() || avatar.source_hash.len() != 64 {
        return Err(Error::InvalidAvatar);
    }
    change.connection().execute(
        "INSERT INTO avatars(id,name,kind,source_hash,image_provider,image_model,voice_provider,voice_id,created_at,created_seq) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,(SELECT COALESCE(MAX(created_seq),0)+1 FROM avatars))",
        params![
            avatar.avatar_id,
            avatar.name,
            avatar.kind,
            avatar.source_hash,
            avatar.image_provider,
            avatar.image_model,
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

/// Removes the avatar. An avatar that a workspace still has is [`Error::AvatarInUse`] and nothing
/// changes: the binding of a closed workspace goes when the workspace closes, so any binding left
/// is an open workspace's. False when the library has no such avatar.
pub fn delete_in(change: &Change<'_>, avatar_id: &str) -> Result<bool> {
    let connection = change.connection();
    let bound: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspace_avatars WHERE avatar_id=?1)",
        [avatar_id],
        |row| row.get(0),
    )?;
    if bound {
        return Err(Error::AvatarInUse);
    }
    let removed = connection.execute("DELETE FROM avatars WHERE id=?1", [avatar_id])?;
    Ok(removed == 1)
}

/// The binding of every open workspace that has an avatar, in the order the workspaces were made.
pub fn bindings(store: &Store) -> Result<Vec<AvatarBinding>> {
    let mut statement = store.connection().prepare(&format!(
        "SELECT {BINDING_COLUMNS} FROM workspace_avatars JOIN workspaces ON workspaces.id = workspace_avatars.workspace_id ORDER BY workspaces.created_seq"
    ))?;
    let rows = statement
        .query_map([], binding_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The binding of one workspace.
pub fn binding(store: &Store, workspace_id: &str) -> Result<Option<AvatarBinding>> {
    Ok(store
        .connection()
        .query_row(
            &format!("SELECT {BINDING_COLUMNS} FROM workspace_avatars WHERE workspace_id=?1"),
            [workspace_id],
            binding_row,
        )
        .optional()?)
}

/// Binds `binding.avatar_id` to the workspace, replacing the binding it had. `binding.revision` is
/// the new one, and the change is made only when it is one past the revision the workspace has now
/// (none is revision 0): a writer that read an older binding is [`Error::StaleAvatarBinding`] and
/// nothing changes.
pub fn bind_in(change: &Change<'_>, binding: &AvatarBinding) -> Result<()> {
    let revision: i64 = binding.revision.parse().map_err(|_| Error::InvalidAvatar)?;
    if binding.harness.is_empty() || revision < 1 {
        return Err(Error::InvalidAvatar);
    }
    let current: Option<i64> = change
        .connection()
        .query_row(
            "SELECT revision FROM workspace_avatars WHERE workspace_id=?1",
            [&binding.workspace_id],
            |row| row.get(0),
        )
        .optional()?;
    if current.unwrap_or(0) + 1 != revision {
        return Err(Error::StaleAvatarBinding);
    }
    change.connection().execute(
        &format!(
            "INSERT INTO workspace_avatars({BINDING_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(workspace_id) DO UPDATE SET avatar_id=excluded.avatar_id, harness=excluded.harness, model=excluded.model, effort=excluded.effort, hearing_provider=excluded.hearing_provider, hearing_language=excluded.hearing_language, voice_provider=excluded.voice_provider, voice_id=excluded.voice_id, persona=excluded.persona, revision=excluded.revision"
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
            revision,
        ],
    )?;
    Ok(())
}
