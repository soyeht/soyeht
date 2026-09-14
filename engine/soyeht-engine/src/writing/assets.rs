use crate::{core::Core, dispatch::Outcome, errors, errors::Result};
use serde_json::{Value, to_value};
use soyeht_protocol::{AttachmentRef, WritingAssetReadParams, WritingAssetReadResult, limits};
use std::path::Path;
use uuid::Uuid;

impl Core {
    pub(crate) fn writing_asset_read(&self, params: &Value) -> Result<Outcome> {
        let request: WritingAssetReadParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing asset read parameters"))?;
        let expected_digest = asset_digest(&request.asset)?;
        let file = self.editor.stored(&request.file_id)?.clone();
        super::documents::require_revision(&request.revision, file.revision)?;
        self.writing_record(&file.file_id)?;
        let workspace = crate::model::workspace(&self.store, &file.workspace_id)?;
        let document_parent = Path::new(&file.relative_path)
            .parent()
            .ok_or_else(|| errors::invalid("Writing document has no containing folder"))?;
        let relative = document_parent.join(&request.asset);
        let relative = relative
            .to_str()
            .ok_or_else(|| errors::invalid("Writing asset path is not UTF-8"))?;
        let root = Path::new(&workspace.path);
        let before = self.editor.resolve(root, relative)?;
        let bytes = self
            .editor
            .read_binary(&root.join(relative), limits::WRITING_IMAGE_BYTES)?;
        let after = self.editor.resolve(root, relative)?;
        if before.canonical != after.canonical
            || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || super::attachments::hex_digest(&bytes) != expected_digest
            || self.editor.stored(&request.file_id)?.revision != file.revision
        {
            return Err(errors::stale_version(
                "Writing asset changed during the bounded read",
            ));
        }
        let reference = AttachmentRef {
            attachment_id: Uuid::new_v4().to_string(),
            mime_type: "image/png".into(),
            name: format!("{expected_digest}.png"),
            bytes: bytes.len().to_string(),
        };
        let result = to_value(WritingAssetReadResult {
            file_id: request.file_id,
            revision: request.revision,
            image: reference.clone(),
        })
        .map_err(|_| errors::internal("Writing asset result cannot be encoded"))?;
        Ok(Outcome::Attachment {
            result,
            reference,
            bytes,
        })
    }
}

fn asset_digest(asset: &str) -> Result<&str> {
    let digest = asset
        .strip_prefix("assets/")
        .and_then(|value| value.strip_suffix(".png"))
        .ok_or_else(|| errors::invalid("Writing asset identifier is invalid"))?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(errors::invalid("Writing asset identifier is invalid"));
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::asset_digest;

    #[test]
    fn asset_identifier_is_exact_lowercase_content_address() {
        let valid = format!("assets/{}.png", "0123456789abcdef".repeat(4));
        assert_eq!(asset_digest(&valid).unwrap(), &valid[7..71]);
        for invalid in [
            "image.png",
            "assets/short.png",
            "assets/ABCDEF0123456789012345678901234567890123456789012345678901234567.png",
            "assets/../../outside.png",
        ] {
            assert!(asset_digest(invalid).is_err(), "{invalid}");
        }
    }
}
