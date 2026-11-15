//! The avatar library, and the avatar a workspace has (ADR 0166, contract 19).
//!
//! An avatar is a reusable record of the person's own: a name, a source picture, a build state and a
//! default voice, with its files in `<data>/avatars/<id>/`. A workspace has at most one avatar, bound
//! with the harness the avatar thinks with, the speech settings, the persona and the clients that
//! have voice on. The engine keeps identifiers and settings: a speech or image provider's key stays
//! in the vault of the device that calls the provider, and a persona is never part of a log line.
//!
//! This module only records and serves. Starting the avatar's pane is the workspace's (slice 2),
//! and running a build is the builder's (slice 9): a build requested here waits as `building`.
use crate::{
    core::Core,
    device_notices::{time_text, viewer},
    dispatch::Outcome,
    errors::{self, Result},
    immutable_file::hex_digest,
    model,
};
use serde_json::{Value, json};
use soyeht_platform::fs::{WriteMode, canonicalize, ensure_private_dir, write_atomic};
use soyeht_protocol::{
    AttachmentRef, Avatar, AvatarAssignParams, AvatarAssignResult, AvatarBinding,
    AvatarBuildParams, AvatarBuildResult, AvatarBuildState, AvatarChangedDraft, AvatarCreateParams,
    AvatarCreateResult, AvatarDeleteParams, AvatarDeleteResult, AvatarListResult,
    AvatarPackageReadParams, AvatarPackageReadResult, AvatarRenameParams, AvatarRenameResult,
    limits, method,
};
use soyeht_store::avatars as library;
use std::{
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// The folder of the data directory that holds one folder per avatar.
const AVATARS_DIRECTORY: &str = "avatars";
/// The source picture, in the avatar's folder.
const SOURCE_FILE: &str = "source.png";
/// The folder of a ready package, in the avatar's folder.
const PACKAGE_DIRECTORY: &str = "package";
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

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
            | method::AVATAR_BUILD
            | method::AVATAR_PACKAGE_READ
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

/// Commits the picture of an `avatar.create` and checks it is a PNG of a size the library takes.
/// The request's identity then names the picture by its content, so a retry that sends the same
/// bytes again repeats the call.
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
    let bytes = client.attachments.commit(&request.picture)?;
    if request.picture.mime_type != "image/png"
        || bytes.is_empty()
        || bytes.len() > limits::AVATAR_PICTURE_BYTES
        || !bytes.starts_with(PNG_SIGNATURE)
    {
        return Err(errors::invalid("An avatar's picture is a PNG file"));
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
        method::AVATAR_PACKAGE_READ => return avatar_package_read(core, params),
        method::AVATAR_LIST => avatar_list(core)?,
        method::AVATAR_CREATE => {
            let picture =
                errors::required(picture, "An avatar is made from a picture it is sent with")?;
            avatar_create(core, connection, params, picture)?
        }
        method::AVATAR_RENAME => avatar_rename(core, connection, params)?,
        method::AVATAR_DELETE => avatar_delete(core, connection, params)?,
        method::AVATAR_ASSIGN => avatar_assign(core, connection, params)?,
        method::AVATAR_BUILD => avatar_build(core, connection, params)?,
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
    if library::count(&core.store).map_err(errors::storage)? >= limits::AVATAR_COUNT {
        return Err(errors::invalid("The avatar library is full"));
    }
    let avatar = Avatar {
        avatar_id: Uuid::new_v4().to_string(),
        name: clean_name(&request.name)?,
        kind: request.kind,
        source_hash: picture.hash,
        build_state: AvatarBuildState::None,
        failure_kind: None,
        package_hash: None,
        voice: request.voice,
        created_at: time_text(core.now_unix())?,
    };
    let result = model::to_value(&AvatarCreateResult {
        avatar: avatar.clone(),
    })?;
    let event = changed(&avatar)?;
    let directory = avatar_directory(core, &avatar.avatar_id);
    write_source(&directory, &picture.bytes)?;
    let made = core.mutate(connection, method::AVATAR_CREATE, params, |tx| {
        library::insert_in(tx, &avatar)?;
        Ok((result, vec![event]))
    });
    match made {
        Ok(mutation) => Ok(mutation.result),
        Err(error) => {
            remove_directory(&directory);
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

fn avatar_delete(core: &mut Core, connection: &str, params: &Value) -> Result<Value> {
    let request: AvatarDeleteParams = model::typed_params(params)?;
    stored_avatar(core, &request.avatar_id)?;
    if !library::open_workspaces_of(&core.store, &request.avatar_id)
        .map_err(errors::storage)?
        .is_empty()
    {
        return Err(errors::pane_busy(
            "Give the workspaces that use this avatar another one first",
        ));
    }
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
    remove_directory(&avatar_directory(core, &request.avatar_id));
    Ok(result)
}

fn avatar_assign(core: &mut Core, connection: &str, params: &Value) -> Result<Value> {
    let request: AvatarAssignParams = model::typed_params(params)?;
    model::workspace(&core.store, &request.workspace_id)?;
    stored_avatar(core, &request.avatar_id)?;
    // Another client's voice is not this call's to change: the clients that have it on are
    // kept, and only the caller's own entry follows `voiceOn`.
    let (me, _) = viewer(core, connection)?;
    let mut voice_on = library::binding(&core.store, &request.workspace_id)
        .map_err(errors::storage)?
        .map(|binding| binding.voice_on)
        .unwrap_or_default();
    match request.voice_on {
        Some(true) if !voice_on.contains(&me) => voice_on.push(me.clone()),
        Some(false) => voice_on.retain(|client| *client != me),
        _ => {}
    }
    voice_on.sort();
    let binding = AvatarBinding {
        workspace_id: request.workspace_id,
        avatar_id: request.avatar_id,
        harness: request.harness,
        model: request.model,
        effort: request.effort,
        hearing: request.hearing,
        voice: request.voice,
        persona: request.persona.unwrap_or_default(),
        voice_on,
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
            if let Some(on) = request.voice_on {
                library::set_voice_in(tx, &binding.workspace_id, &me, on)?;
            }
            Ok((result, vec![event]))
        })?
        .result)
}

/// Asks for a build. An avatar with nothing built, or whose last build failed, becomes
/// `building`; one already building stays so and is answered as it is. A ready avatar is
/// finished: another picture is another avatar.
fn avatar_build(core: &mut Core, connection: &str, params: &Value) -> Result<Value> {
    let request: AvatarBuildParams = model::typed_params(params)?;
    let mut avatar = stored_avatar(core, &request.avatar_id)?;
    match avatar.build_state {
        AvatarBuildState::Building => {
            return model::to_value(&AvatarBuildResult { avatar });
        }
        AvatarBuildState::Ready => {
            return Err(errors::transition_refused(
                "The avatar is built; another picture makes another avatar",
            ));
        }
        _ => {}
    }
    avatar.build_state = AvatarBuildState::Building;
    avatar.failure_kind = None;
    avatar.package_hash = None;
    let result = model::to_value(&AvatarBuildResult {
        avatar: avatar.clone(),
    })?;
    let event = changed(&avatar)?;
    Ok(core
        .mutate(connection, method::AVATAR_BUILD, params, |tx| {
            library::start_build_in(tx, &avatar.avatar_id)?;
            Ok((result, vec![event]))
        })?
        .result)
}

/// One chunk of one file of a ready package, so that a phone can fetch it in pieces.
pub(crate) fn avatar_package_read(core: &Core, params: &Value) -> Result<Outcome> {
    let request: AvatarPackageReadParams = model::typed_params(params)?;
    let avatar = stored_avatar(core, &request.avatar_id)?;
    if avatar.build_state != AvatarBuildState::Ready {
        return Err(errors::not_found("The avatar has no package"));
    }
    let offset = errors::required(
        request.offset.parse::<u64>().ok(),
        "Package offset is not a number",
    )?;
    let root = avatar_directory(core, &avatar.avatar_id).join(PACKAGE_DIRECTORY);
    let (name, size, bytes) = read_chunk(&root, &request.file, offset, request.length)?;
    let reference = AttachmentRef {
        attachment_id: Uuid::new_v4().to_string(),
        mime_type: "application/octet-stream".into(),
        name,
        bytes: bytes.len().to_string(),
    };
    let result = model::to_value(&AvatarPackageReadResult {
        avatar_id: avatar.avatar_id,
        file: request.file,
        offset: request.offset,
        size: size.to_string(),
        chunk: reference.clone(),
    })?;
    Ok(Outcome::Attachment {
        result,
        reference,
        bytes,
    })
}

/// Makes the avatar's folder, owner-only, and writes its source picture in it.
fn write_source(directory: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = directory.parent() {
        ensure_private_dir(parent).map_err(|_| errors::internal("Avatar folder is not private"))?;
    }
    ensure_private_dir(directory).map_err(|_| errors::internal("Avatar folder is not private"))?;
    write_atomic(&directory.join(SOURCE_FILE), bytes, WriteMode::OwnerOnly).map_err(errors::io)
}

/// Removes an avatar's folder. A failure is logged by kind: nothing refers to the folder any more,
/// and what stays is the person's own disk space.
fn remove_directory(directory: &Path) {
    if let Err(error) = std::fs::remove_dir_all(directory)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        soyeht_platform::log::failure("engine.avatar_folder_removal_failed", &error);
    }
}

/// The path of `file` in the package folder `root`, when `file` names one regular file inside it:
/// written with `/`, with no empty, `.` or `..` part, and not through a link that leaves the folder.
fn package_file(root: &Path, file: &str) -> Result<PathBuf> {
    if file.contains(['\\', ':'])
        || file.starts_with('/')
        || file.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(errors::invalid("Package file name is not a relative path"));
    }
    let missing = || errors::not_found("The package has no such file");
    let root = canonicalize(root).map_err(|_| missing())?;
    let target = canonicalize(&root.join(file)).map_err(|_| missing())?;
    if !target.starts_with(&root) || !target.is_file() {
        return Err(missing());
    }
    Ok(target)
}

/// The last name of `file`, its size and the bytes of the chunk at `offset`: at most `length`, and
/// at most one attachment frame.
fn read_chunk(
    root: &Path,
    file: &str,
    offset: u64,
    length: Option<u32>,
) -> Result<(String, u64, Vec<u8>)> {
    let target = package_file(root, file)?;
    let mut open = File::open(&target).map_err(errors::io)?;
    let size = open.metadata().map_err(errors::io)?.len();
    if offset > size {
        return Err(errors::invalid(
            "Package offset is past the end of the file",
        ));
    }
    let wanted = u64::from(length.unwrap_or(u32::MAX))
        .min(limits::ATTACHMENT_PAYLOAD_BYTES as u64)
        .min(size - offset);
    open.seek(SeekFrom::Start(offset)).map_err(errors::io)?;
    let mut bytes = Vec::new();
    open.take(wanted)
        .read_to_end(&mut bytes)
        .map_err(errors::io)?;
    let name = file.rsplit('/').next().unwrap_or(file).to_owned();
    Ok((name, size, bytes))
}

#[cfg(test)]
mod tests;
