use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};

use soyeht_protocol::{AttachmentRef, WritingFormatKind, WritingFormatParams, limits};
use std::path::Path;

pub(crate) struct WritingImageAttachment {
    pub(crate) workspace_relative_path: String,
    pub(crate) bytes: Vec<u8>,
}

impl Core {
    pub(crate) fn prepare_writing_image(
        &mut self,
        connection: &str,
        params: &mut Value,
    ) -> Result<Option<WritingImageAttachment>> {
        let request: WritingFormatParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing image parameters"))?;
        let Some(reference) = request.image else {
            return Ok(None);
        };
        if request.kind != WritingFormatKind::Image || request.value.is_some() {
            return Err(errors::invalid(
                "Binary writing images require image format without a path",
            ));
        }
        let file = self.editor.stored(&request.file_id)?;
        let parent = Path::new(&file.relative_path)
            .parent()
            .ok_or_else(|| errors::invalid("Writing document has no containing folder"))?
            .to_owned();
        let bytes = self.commit_writing_image(connection, &reference)?;
        let digest = crate::immutable_file::hex_digest(&bytes);
        let markdown_path = format!("assets/{digest}.png");
        let workspace_relative_path = parent.join(&markdown_path);
        let workspace_relative_path = workspace_relative_path
            .to_str()
            .ok_or_else(|| errors::invalid("Writing image path is not UTF-8"))?
            .to_owned();
        let object = params
            .as_object_mut()
            .ok_or_else(|| errors::invalid("Writing format parameters must be an object"))?;
        object.insert("value".into(), json!(markdown_path));
        object.insert("_attachmentDigest".into(), json!(digest));
        Ok(Some(WritingImageAttachment {
            workspace_relative_path,
            bytes,
        }))
    }

    fn commit_writing_image(
        &mut self,
        connection: &str,
        reference: &AttachmentRef,
    ) -> Result<Vec<u8>> {
        let client = self
            .clients
            .get_mut(connection)
            .ok_or_else(|| errors::unauthorized("Attachment connection is unknown"))?;
        let bytes = client.attachments.commit(reference)?;
        if reference.mime_type != "image/png"
            || reference.name.is_empty()
            || reference.name.len() > 256
            || bytes.is_empty()
            || bytes.len() > limits::WRITING_IMAGE_BYTES
            || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || reference.bytes != bytes.len().to_string()
        {
            return Err(errors::invalid("Invalid writing image attachment"));
        }
        Ok(bytes)
    }
}
