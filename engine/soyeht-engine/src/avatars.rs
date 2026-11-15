//! The avatar library, and the avatar a workspace has (ADR 0166, contract 19).
//!
//! An avatar is a reusable record of the person's own: a name, a source picture, the catalogue ids
//! of the image provider and model, and a default voice, with its files in `<data>/avatars/<id>/`.
//! A workspace has at most one avatar, bound with the harness the avatar thinks with, the speech
//! settings and the persona, under a revision: a client that read an older binding cannot overwrite
//! a newer one. The engine keeps identifiers and settings: a speech or image provider's key stays in
//! the vault of the device that calls the provider, whether a client has voice on stays with that
//! client, and a persona is never part of a log line.
//!
//! This module only records. Starting the avatar's pane is the workspace's (slice 2), and building
//! the avatar is the builder's (slice 9), which brings its own calls.
use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    immutable_file::hex_digest,
    model, png,
};
use serde_json::{Value, json};
use soyeht_platform::fs::{WriteMode, ensure_private_dir, write_atomic};
use soyeht_protocol::{
    Avatar, AvatarAssignParams, AvatarAssignResult, AvatarBinding, AvatarChangedDraft,
    AvatarCreateParams, AvatarCreateResult, AvatarDeleteParams, AvatarDeleteResult,
    AvatarListResult, AvatarRenameParams, AvatarRenameResult, limits, method,
};
use soyeht_store::{avatars as library, database::Store};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The folder of the data directory that holds one folder per avatar.
const AVATARS_DIRECTORY: &str = "avatars";
/// The source picture, in the avatar's folder.
const SOURCE_FILE: &str = "source.png";
/// The kinds `avatar.create` takes. `preset` has no producer yet: presets come with the preset pack.
const CREATABLE_KINDS: [&str; 2] = ["uploaded", "generated"];

/// A source picture that was sent with `avatar.create` and checked, with its hash.
pub(crate) struct AvatarPicture {
    bytes: Vec<u8>,
    hash: String,
}

/// Whether `method` is one of this module's.
pub(crate) fn handles(method: &str) -> bool {
    matches!(
        method,
        method::AVATAR_LIST
            | method::AVATAR_CREATE
            | method::AVATAR_RENAME
            | method::AVATAR_DELETE
            | method::AVATAR_ASSIGN
    )
}

/// The name as it is kept: the person's, without the space around it. A name with nothing in it is
/// refused.
fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(errors::invalid("An avatar needs a name"));
    }
    Ok(name.to_owned())
}

fn changed(avatar: &Avatar) -> Result<(String, Value)> {
    model::pending(&AvatarChangedDraft {
        avatar_id: avatar.avatar_id.clone(),
        avatar: Some(avatar.clone()),
        workspace_id: None,
        binding: None,
    })
}

/// Commits the picture of an `avatar.create` and checks it is a PNG of a size the library takes:
/// its header must be well formed, and neither side may pass `avatarPictureDimension`. The
/// request's identity then names the picture by its content, so a retry that sends the same bytes
/// again repeats the call.
pub(crate) fn prepare_avatar_picture(
    core: &mut Core,
    connection: &str,
    params: &mut Value,
) -> Result<AvatarPicture> {
    let request: AvatarCreateParams = model::typed_params(params)?;
    let client = errors::invariant(
        core.clients.get_mut(connection),
        "Attachment connection is unknown",
    )?;
    let bytes = client
        .attachments
        .commit(&request.picture)
        .map_err(|error| {
            crate::agents::attachments::upload_failure(
                error,
                format!(
                    "A picture of an avatar is at most {} MiB",
                    limits::AVATAR_PICTURE_BYTES / (1024 * 1024)
                ),
            )
        })?;
    if bytes.len() > limits::AVATAR_PICTURE_BYTES {
        return Err(errors::attachment_limit_exceeded(format!(
            "A picture of an avatar is at most {} MiB",
            limits::AVATAR_PICTURE_BYTES / (1024 * 1024)
        )));
    }
    let side = u32::try_from(limits::AVATAR_PICTURE_DIMENSION).unwrap_or(u32::MAX);
    let sized =
        png::dimensions(&bytes).is_some_and(|(width, height)| width <= side && height <= side);
    if request.picture.mime_type != png::MIME || !sized {
        return Err(errors::invalid(format!(
            "An avatar's picture is a PNG file of at most {} pixels a side",
            limits::AVATAR_PICTURE_DIMENSION
        )));
    }
    let hash = hex_digest(&bytes);
    let picture = errors::required(
        params.get_mut("picture").and_then(Value::as_object_mut),
        "An avatar is made from a picture it is sent with",
    )?;
    picture.insert("attachmentId".into(), json!(hash));
    Ok(AvatarPicture { bytes, hash })
}

pub(crate) fn avatar_request(
    core: &mut Core,
    connection: &str,
    method: &str,
    params: &Value,
    picture: Option<AvatarPicture>,
) -> Result<Outcome> {
    let result = match method {
        method::AVATAR_LIST => avatar_list(core)?,
        method::AVATAR_CREATE => {
            let picture =
                errors::required(picture, "An avatar is made from a picture it is sent with")?;
            avatar_create(core, connection, params, picture)?
        }
        method::AVATAR_RENAME => avatar_rename(core, connection, params)?,
        method::AVATAR_DELETE => avatar_delete(core, connection, params)?,
        method::AVATAR_ASSIGN => avatar_assign(core, connection, params)?,
        _ => return Err(errors::internal("Method is not an avatar method")),
    };
    Ok(Outcome::Reply(result))
}

fn avatar_directory(core: &Core, avatar_id: &str) -> PathBuf {
    core.directory.join(AVATARS_DIRECTORY).join(avatar_id)
}

fn stored_avatar(core: &Core, avatar_id: &str) -> Result<Avatar> {
    library::get(&core.store, avatar_id)
        .map_err(errors::storage)?
        .ok_or_else(|| errors::not_found("Avatar not found"))
}

fn avatar_list(core: &Core) -> Result<Value> {
    model::to_value(&AvatarListResult {
        avatars: library::list(&core.store).map_err(errors::storage)?,
        bindings: library::bindings(&core.store).map_err(errors::storage)?,
    })
}

fn avatar_create(
    core: &mut Core,
    connection: &str,
    params: &Value,
    picture: AvatarPicture,
) -> Result<Value> {
    let request: AvatarCreateParams = model::typed_params(params)?;
    if !CREATABLE_KINDS.contains(&request.kind.as_str()) {
        return Err(errors::invalid(
            "An avatar's kind is `uploaded` or `generated`",
        ));
    }
    if library::count(&core.store).map_err(errors::storage)? >= limits::AVATAR_COUNT {
        return Err(errors::invalid("The avatar library is full"));
    }
    let avatar = Avatar {
        avatar_id: Uuid::new_v4().to_string(),
        name: clean_name(&request.name)?,
        kind: request.kind,
        source_hash: picture.hash,
        image_provider: request.image_provider,
        image_model: request.image_model,
        voice: request.voice,
        created_at: crate::device_notices::time_text(core.now_unix())?,
    };
    let result = model::to_value(&AvatarCreateResult {
        avatar: avatar.clone(),
    })?;
    let event = changed(&avatar)?;
    write_source(&avatar_directory(core, &avatar.avatar_id), &picture.bytes)?;
    let made = core.mutate(connection, method::AVATAR_CREATE, params, |tx| {
        library::insert_in(tx, &avatar)?;
        Ok((result, vec![event]))
    });
    match made {
        Ok(mutation) => Ok(mutation.result),
        Err(error) => {
            remove_folder(core, &avatar.avatar_id);
            Err(error)
        }
    }
}

fn avatar_rename(core: &mut Core, connection: &str, params: &Value) -> Result<Value> {
    let request: AvatarRenameParams = model::typed_params(params)?;
    let mut avatar = stored_avatar(core, &request.avatar_id)?;
    avatar.name = clean_name(&request.name)?;
    let result = model::to_value(&AvatarRenameResult {
        avatar: avatar.clone(),
    })?;
    let event = changed(&avatar)?;
    Ok(core
        .mutate(connection, method::AVATAR_RENAME, params, |tx| {
            library::rename_in(tx, &avatar.avatar_id, &avatar.name)?;
            Ok((result, vec![event]))
        })?
        .result)
}

/// Removes the avatar. One that an open workspace has is refused by the store (`pane_busy`).
fn avatar_delete(core: &mut Core, connection: &str, params: &Value) -> Result<Value> {
    let request: AvatarDeleteParams = model::typed_params(params)?;
    stored_avatar(core, &request.avatar_id)?;
    let result = model::to_value(&AvatarDeleteResult {
        avatar_id: request.avatar_id.clone(),
        removed: true,
    })?;
    let event = model::pending(&AvatarChangedDraft {
        avatar_id: request.avatar_id.clone(),
        avatar: None,
        workspace_id: None,
        binding: None,
    })?;
    let result = core
        .mutate(connection, method::AVATAR_DELETE, params, |tx| {
            library::delete_in(tx, &request.avatar_id)?;
            Ok((result, vec![event]))
        })?
        .result;
    remove_folder(core, &request.avatar_id);
    Ok(result)
}

/// Gives an open workspace its avatar, replacing the binding it had. The caller names the revision
/// it read (`0` for none); another is `stale_version`, so a client with an old copy never overwrites
/// a newer one.
fn avatar_assign(core: &mut Core, connection: &str, params: &Value) -> Result<Value> {
    let request: AvatarAssignParams = model::typed_params(params)?;
    model::workspace(&core.store, &request.workspace_id)?;
    stored_avatar(core, &request.avatar_id)?;
    let current = library::binding(&core.store, &request.workspace_id)
        .map_err(errors::storage)?
        .map_or(Ok(0), |binding| binding.revision.parse::<u64>())
        .map_err(|_| errors::internal("A stored binding has no revision"))?;
    let base = request
        .base_revision
        .parse::<u64>()
        .map_err(|_| errors::invalid("`baseRevision` is not a number"))?;
    if base != current {
        return Err(errors::stale_version(
            "The workspace's avatar changed since it was read",
        ));
    }
    let binding = AvatarBinding {
        workspace_id: request.workspace_id,
        avatar_id: request.avatar_id,
        harness: request.harness,
        model: request.model,
        effort: request.effort,
        hearing: request.hearing,
        voice: request.voice,
        persona: request.persona.unwrap_or_default(),
        revision: (current + 1).to_string(),
    };
    let result = model::to_value(&AvatarAssignResult {
        binding: binding.clone(),
    })?;
    let event = model::pending(&AvatarChangedDraft {
        avatar_id: binding.avatar_id.clone(),
        avatar: None,
        workspace_id: Some(binding.workspace_id.clone()),
        binding: Some(binding.clone()),
    })?;
    Ok(core
        .mutate(connection, method::AVATAR_ASSIGN, params, |tx| {
            library::bind_in(tx, &binding)?;
            Ok((result, vec![event]))
        })?
        .result)
}

/// Makes the avatar's folder, owner-only, and writes its source picture in it.
fn write_source(directory: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = directory.parent() {
        ensure_private_dir(parent).map_err(|_| errors::internal("Avatar folder is not private"))?;
    }
    ensure_private_dir(directory).map_err(|_| errors::internal("Avatar folder is not private"))?;
    write_atomic(&directory.join(SOURCE_FILE), bytes, WriteMode::OwnerOnly).map_err(errors::io)
}

/// Removes an avatar's folder off the Core lock (ADR 0079): it is a walk of a tree. A failure is
/// logged by kind: nothing refers to the folder any more, and what stays is the person's own disk
/// space, which the next start sweeps.
fn remove_folder(core: &mut Core, avatar_id: &str) {
    let folder = avatar_directory(core, avatar_id);
    let started = core.spawn_background(
        format!("avatar-folder:{avatar_id}"),
        move || remove_tree(&folder),
        |_, removed| {
            if let Err(error) = &removed {
                soyeht_platform::log::failure("engine.avatar_folder_removal_failed", error);
            }
            Ok(())
        },
    );
    if let Err(error) = started {
        soyeht_platform::log::failure("engine.avatar_folder_removal_failed", &error);
    }
}

fn remove_tree(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(errors::io(error)),
        _ => Ok(()),
    }
}

/// Removes what `<data>/avatars/` holds that the library does not know: the folder of an avatar
/// whose creation did not commit, or whose removal did not finish. It runs as the engine opens, before
/// the Core is shared, so no request waits for the walk.
pub(crate) fn sweep_orphans(directory: &Path, store: &Store) {
    let root = directory.join(AVATARS_DIRECTORY);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let known = match library::ids(store) {
        Ok(known) => known,
        Err(error) => {
            soyeht_platform::log::failure("engine.avatar_sweep_failed", &errors::storage(error));
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|name| known.iter().any(|id| id == name))
        {
            continue;
        }
        let path = entry.path();
        let removed = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(error) = removed {
            soyeht_platform::log::failure("engine.avatar_sweep_failed", &error);
        }
    }
}

#[cfg(test)]
mod tests;
