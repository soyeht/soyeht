//! Prompt attachments. The engine checks what a client attached, stages sent
//! bytes as content-addressed files it owns, and queues the prompt with
//! absolute paths only. The ACP block each file becomes is chosen at delivery
//! from the agent's advertised capabilities, in `soyeht_acp::prompt`.
use crate::{
    core::Core,
    errors::{self, Result},
    immutable_file,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::{
    AttachmentRef, PanePromptParams, PromptAttachment, PromptAttachmentKind, limits,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Engine-owned directory, under the data directory, for staged bytes.
const STAGING_DIRECTORY: &str = "prompt-attachments";
const MEBIBYTE: usize = 1024 * 1024;

/// An accepted attachment. Sent bytes are written to `attachment.path` by
/// `publish`, after the prompt is accepted, so a refused prompt stages nothing.
pub struct PreparedAttachment {
    pub(crate) attachment: PromptAttachment,
    bytes: Option<Vec<u8>>,
}

impl PreparedAttachment {
    pub(crate) fn publish(&self) -> Result<()> {
        let (Some(bytes), Some(path)) = (&self.bytes, &self.attachment.path) else {
            return Ok(());
        };
        let target = Path::new(path);
        if let Some(directory) = target.parent() {
            fs::create_dir_all(directory)?;
        }
        immutable_file::publish(target, bytes).map_err(errors::io)
    }
}

impl Core {
    /// Validates the attachments of a `pane.prompt` request and rewrites them in
    /// `params` as staged paths. The mutation identity then names content by
    /// digest: a retry with the same bytes repeats, different bytes conflict.
    pub(crate) fn prepare_prompt_attachments(
        &mut self,
        connection: &str,
        params: &mut Value,
    ) -> Result<Vec<PreparedAttachment>> {
        let prompt: PanePromptParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid prompt attachment parameters"))?;
        let requested = prompt.attachments.unwrap_or_default();
        let uploads: Vec<_> = requested.iter().filter_map(upload_reference).collect();
        let client = self
            .clients
            .get_mut(connection)
            .ok_or_else(|| errors::unauthorized("Attachment connection is unknown"))?;
        // Committing first leaves the connection ready for its next request
        // whatever this one's outcome.
        let mut uploaded = client
            .attachments
            .commit_all(&uploads)
            .map_err(upload_error)?
            .into_iter();
        if requested.len() > limits::PROMPT_ATTACHMENTS {
            return Err(errors::attachment_limit_exceeded(format!(
                "A prompt takes at most {} attachments",
                limits::PROMPT_ATTACHMENTS
            )));
        }
        let staging = self.directory.join(STAGING_DIRECTORY);
        let mut total = 0usize;
        let mut prepared = Vec::with_capacity(requested.len());
        for attachment in requested {
            validate_description(&attachment)?;
            let bytes = match (
                &attachment.attachment_id,
                &attachment.data,
                &attachment.path,
            ) {
                (Some(_), None, None) => Some(errors::required(
                    uploaded.next(),
                    "Attachment transfer is missing",
                )?),
                (None, Some(data), None) => Some(inline_bytes(data)?),
                (None, None, Some(_)) => None,
                _ => {
                    return Err(errors::invalid(
                        "Each attachment needs exactly one of attachmentId, data or path",
                    ));
                }
            };
            let item = match bytes {
                Some(bytes) => {
                    total = total.saturating_add(bytes.len());
                    staged(attachment, bytes, &staging, total)?
                }
                None => host_file(attachment)?,
            };
            prepared.push(item);
        }
        let staged: Vec<_> = prepared.iter().map(|item| &item.attachment).collect();
        params
            .as_object_mut()
            .ok_or_else(|| errors::invalid("Prompt parameters must be an object"))?
            .insert("attachments".into(), json!(staged));
        Ok(prepared)
    }
}

fn upload_reference(attachment: &PromptAttachment) -> Option<AttachmentRef> {
    Some(AttachmentRef {
        attachment_id: attachment.attachment_id.clone()?,
        mime_type: attachment.mime_type.clone(),
        name: attachment.name.clone(),
        bytes: attachment.bytes.clone(),
    })
}

fn upload_error(error: soyeht_protocol::Error) -> errors::Error {
    match error {
        soyeht_protocol::Error::AttachmentTooLarge => errors::attachment_limit_exceeded(format!(
            "Attachments are limited to {} files and {} MiB per prompt",
            limits::PROMPT_ATTACHMENTS,
            limits::PROMPT_ATTACHMENT_TOTAL_BYTES / MEBIBYTE
        )),
        error => errors::invalid(format!("Attachment transfer failed: {error}")),
    }
}

fn validate_description(attachment: &PromptAttachment) -> Result<()> {
    let mime = attachment.mime_type.as_str();
    let well_formed = mime.split_once('/').is_some_and(|(kind, subtype)| {
        !kind.is_empty()
            && !subtype.is_empty()
            && !subtype.contains('/')
            && mime.chars().all(|c| c.is_ascii_graphic())
    });
    let consistent = match attachment.kind {
        PromptAttachmentKind::Image => mime.starts_with("image/"),
        PromptAttachmentKind::Audio => mime.starts_with("audio/"),
        PromptAttachmentKind::Text | PromptAttachmentKind::File => true,
    };
    if !well_formed || !consistent {
        return Err(errors::invalid(format!(
            "Attachment {} has an invalid MIME type for its kind",
            attachment.name
        )));
    }
    if attachment.name.chars().any(char::is_control) {
        return Err(errors::invalid("Attachment names cannot contain controls"));
    }
    if let Some(thumbnail) = &attachment.thumbnail {
        let image = STANDARD.decode(thumbnail).unwrap_or_default();
        let picture = image.starts_with(b"\x89PNG\r\n\x1a\n") || image.starts_with(b"\xff\xd8\xff");
        if !picture || image.len() > limits::PROMPT_ATTACHMENT_THUMBNAIL_BYTES {
            return Err(errors::invalid(format!(
                "An attachment thumbnail must be a PNG or JPEG of at most {} KiB",
                limits::PROMPT_ATTACHMENT_THUMBNAIL_BYTES / 1024
            )));
        }
    }
    Ok(())
}

fn inline_bytes(data: &str) -> Result<Vec<u8>> {
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| errors::invalid("Inline attachment data is not base64"))?;
    if bytes.len() > limits::PROMPT_ATTACHMENT_INLINE_BYTES {
        return Err(errors::attachment_limit_exceeded(format!(
            "Inline attachment data is limited to {} KiB; send larger files as binary frames",
            limits::PROMPT_ATTACHMENT_INLINE_BYTES / 1024
        )));
    }
    Ok(bytes)
}

/// Bytes the client sent, checked against their declaration and the limits,
/// and named `<staging>/<sha256>/<name>`.
fn staged(
    attachment: PromptAttachment,
    bytes: Vec<u8>,
    staging: &Path,
    total: usize,
) -> Result<PreparedAttachment> {
    if bytes.is_empty() || attachment.bytes != bytes.len().to_string() {
        return Err(errors::invalid(format!(
            "Attachment {} does not match its declared size",
            attachment.name
        )));
    }
    if bytes.len() > limits::PROMPT_ATTACHMENT_BYTES {
        return Err(errors::attachment_limit_exceeded(format!(
            "{} is larger than the {} MiB attachment limit",
            attachment.name,
            limits::PROMPT_ATTACHMENT_BYTES / MEBIBYTE
        )));
    }
    if total > limits::PROMPT_ATTACHMENT_TOTAL_BYTES {
        return Err(errors::attachment_limit_exceeded(format!(
            "Attachments are limited to {} MiB per prompt",
            limits::PROMPT_ATTACHMENT_TOTAL_BYTES / MEBIBYTE
        )));
    }
    let path = staging
        .join(immutable_file::hex_digest(&bytes))
        .join(file_name(&attachment.name));
    Ok(PreparedAttachment {
        attachment: located(attachment, &path, bytes.len() as u64)?,
        bytes: Some(bytes),
    })
}

/// A file already on the engine host. It is linked where it is, not copied,
/// so only the attachment count limits it.
fn host_file(attachment: PromptAttachment) -> Result<PreparedAttachment> {
    let unreadable = || {
        errors::invalid(format!(
            "Attachment {} is not a readable file on this Mac",
            attachment.name
        ))
    };
    let path = attachment
        .path
        .as_deref()
        .map(Path::new)
        .unwrap_or(Path::new(""));
    if !path.is_absolute() {
        return Err(unreadable());
    }
    let canonical = soyeht_platform::fs::canonicalize(path).map_err(|_| unreadable())?;
    let metadata = fs::metadata(&canonical).map_err(|_| unreadable())?;
    if !metadata.is_file() || fs::File::open(&canonical).is_err() {
        return Err(unreadable());
    }
    Ok(PreparedAttachment {
        attachment: located(attachment, &canonical, metadata.len())?,
        bytes: None,
    })
}

fn located(attachment: PromptAttachment, path: &Path, bytes: u64) -> Result<PromptAttachment> {
    let path = path
        .to_str()
        .ok_or_else(|| errors::invalid("Attachment path is not UTF-8"))?
        .to_owned();
    Ok(PromptAttachment {
        bytes: bytes.to_string(),
        attachment_id: None,
        data: None,
        path: Some(path),
        ..attachment
    })
}

/// A safe single file name that keeps the user's name and extension where it
/// can, so the agent sees a meaningful path.
fn file_name(name: &str) -> PathBuf {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || c == ':' { '_' } else { c })
        .collect();
    let mut cleaned = cleaned.trim().trim_start_matches('.').to_owned();
    while cleaned.len() > 200 {
        cleaned.pop();
    }
    PathBuf::from(if cleaned.is_empty() {
        "attachment".to_owned()
    } else {
        cleaned
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(kind: PromptAttachmentKind, mime: &str) -> PromptAttachment {
        PromptAttachment {
            kind,
            name: "shot.png".into(),
            mime_type: mime.into(),
            bytes: "3".into(),
            attachment_id: None,
            data: Some("AAAA".into()),
            path: None,
            thumbnail: None,
        }
    }

    #[test]
    fn file_names_stay_single_visible_components() {
        assert_eq!(file_name("../../etc/passwd"), PathBuf::from("passwd"));
        assert_eq!(file_name(".hidden"), PathBuf::from("hidden"));
        assert_eq!(file_name("a\u{7}b:c.txt"), PathBuf::from("a_b_c.txt"));
        assert_eq!(file_name("..."), PathBuf::from("attachment"));
        assert!(file_name(&"é".repeat(150)).as_os_str().len() <= 200);
    }

    #[test]
    fn descriptions_must_agree_with_their_kind_and_carry_a_small_picture() {
        assert!(
            validate_description(&attachment(PromptAttachmentKind::Image, "image/png")).is_ok()
        );
        assert!(
            validate_description(&attachment(PromptAttachmentKind::Image, "text/plain")).is_err()
        );
        assert!(
            validate_description(&attachment(PromptAttachmentKind::Audio, "audio/wav")).is_ok()
        );
        assert!(validate_description(&attachment(PromptAttachmentKind::File, "pdf")).is_err());
        assert!(validate_description(&attachment(PromptAttachmentKind::File, "a/b c")).is_err());
        let mut thumbnail = attachment(PromptAttachmentKind::Image, "image/png");
        thumbnail.thumbnail = Some(STANDARD.encode(b"\xff\xd8\xffjpeg"));
        assert!(validate_description(&thumbnail).is_ok());
        thumbnail.thumbnail = Some(STANDARD.encode(b"not a picture"));
        assert!(validate_description(&thumbnail).is_err());
    }

    #[test]
    fn sizes_are_checked_against_the_declaration_and_both_limits() {
        let staging = crate::test_paths::rooted("/staging");
        let staging = staging.as_path();
        let image = attachment(PromptAttachmentKind::Image, "image/png");
        let item = staged(image.clone(), b"abc".to_vec(), staging, 3).unwrap();
        let path = item.attachment.path.unwrap();
        assert!(path.starts_with(&format!(
            "{}{}ba7816bf",
            staging.display(),
            std::path::MAIN_SEPARATOR
        )));
        assert!(path.ends_with(&format!("{}shot.png", std::path::MAIN_SEPARATOR)));
        assert!(item.attachment.data.is_none());
        assert!(staged(image.clone(), b"ab".to_vec(), staging, 2).is_err());
        let total = staged(
            image,
            b"abc".to_vec(),
            staging,
            limits::PROMPT_ATTACHMENT_TOTAL_BYTES + 1,
        );
        assert!(matches!(
            total.map(|_| ()),
            Err(errors::Error::AttachmentLimitExceeded(_))
        ));
        assert!(matches!(
            inline_bytes(&STANDARD.encode(vec![0; limits::PROMPT_ATTACHMENT_INLINE_BYTES + 1])),
            Err(errors::Error::AttachmentLimitExceeded(_))
        ));
    }
}
