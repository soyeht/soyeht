//! Turns picked, dropped and pasted content into attachment drafts.

use super::{Draft, Rejection, Source, classify, image_prep, image_prep::Prepared};
use soyeht_protocol::{PromptAttachmentKind, limits::PROMPT_ATTACHMENT_BYTES};
use std::path::Path;

/// A file picked or dropped. Images are always sent as prepared bytes; another file is linked in
/// place when the engine runs on this machine (`link_host_files`), else sent as bytes.
pub fn stage_path(path: &Path, link_host_files: bool) -> Result<Draft, Rejection> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let unreadable = || Rejection::Unreadable { name: name.clone() };
    let metadata = std::fs::metadata(path).map_err(|_| unreadable())?;
    if metadata.is_dir() {
        return Err(Rejection::Folder { name });
    }
    let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    let (kind, mime_type) = classify(&name);
    if kind == PromptAttachmentKind::Image
        && size <= PROMPT_ATTACHMENT_BYTES
        && let Ok(data) = std::fs::read(path)
        && let Some(draft) = stage_image(&data, &mime_type, Some(&name))
    {
        return Ok(draft);
    }
    if link_host_files {
        let canonical = soyeht_platform::fs::canonicalize(path).map_err(|_| unreadable())?;
        return Ok(Draft::new(
            kind,
            name,
            mime_type,
            size,
            Source::HostFile(canonical.to_string_lossy().into_owned()),
            None,
        ));
    }
    if size > PROMPT_ATTACHMENT_BYTES {
        return Err(Rejection::TooLarge { name });
    }
    let data = std::fs::read(path).map_err(|_| unreadable())?;
    Ok(Draft::new(
        kind,
        name,
        mime_type,
        size,
        Source::Bytes(data),
        None,
    ))
}

/// Bytes with a known name, such as a file a clipboard provides by value.
pub fn stage_bytes(data: Vec<u8>, name: &str) -> Draft {
    let (kind, mime_type) = classify(name);
    if kind == PromptAttachmentKind::Image
        && let Some(draft) = stage_image(&data, &mime_type, Some(name))
    {
        return draft;
    }
    let size = data.len();
    Draft::new(
        kind,
        name.to_owned(),
        mime_type,
        size,
        Source::Bytes(data),
        None,
    )
}

/// An image from the clipboard, a drop or the photo library, prepared for the agent with a
/// thumbnail. `None` when the bytes are not a readable image.
pub fn stage_image(data: &[u8], mime_type: &str, name: Option<&str>) -> Option<Draft> {
    let Prepared {
        bytes,
        mime_type,
        thumbnail,
    } = image_prep::prepare(data, mime_type)?;
    Some(Draft::new(
        PromptAttachmentKind::Image,
        image_prep::image_file_name(name, mime_type),
        mime_type.to_owned(),
        bytes.len(),
        Source::Bytes(bytes),
        thumbnail,
    ))
}
