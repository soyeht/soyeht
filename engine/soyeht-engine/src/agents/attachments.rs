use crate::{
    core::Core,
    errors::{self, Result},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{AttachmentRef, PanePromptParams, limits};
use soyeht_store::agents::PromptAttachment;

fn validate(reference: &AttachmentRef, bytes: &[u8]) -> Result<()> {
    if reference.mime_type != "image/png"
        || reference.name.is_empty()
        || reference.name.len() > 256
        || bytes.is_empty()
        || reference.bytes != bytes.len().to_string()
    {
        return Err(errors::invalid("Invalid prompt image attachment"));
    }
    Ok(())
}

impl Core {
    pub(crate) fn prepare_prompt_attachments(
        &mut self,
        connection: &str,
        params: &mut Value,
    ) -> Result<Vec<PromptAttachment>> {
        let prompt: PanePromptParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid prompt attachment parameters"))?;
        let references = prompt.attachments.unwrap_or_default();
        if references.len() > limits::WEB_PICKED_ELEMENTS {
            return Err(errors::invalid("Too many prompt image attachments"));
        }
        let client = self
            .clients
            .get_mut(connection)
            .ok_or_else(|| errors::unauthorized("Attachment connection is unknown"))?;
        let bytes = client.attachments.commit_all(&references)?;
        let mut attachments = Vec::with_capacity(references.len());
        let mut digests = Vec::with_capacity(references.len());
        for (reference, bytes) in references.into_iter().zip(bytes) {
            validate(&reference, &bytes)?;
            digests.push(STANDARD.encode(Sha256::digest(&bytes)));
            attachments.push(PromptAttachment { reference, bytes });
        }
        if !digests.is_empty() {
            params
                .as_object_mut()
                .ok_or_else(|| errors::invalid("Prompt parameters must be an object"))?
                .insert("_attachmentDigests".into(), json!(digests));
        }
        Ok(attachments)
    }
}
