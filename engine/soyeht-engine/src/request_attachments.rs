//! The binary attachments a request carried, committed before its stored result is looked up.
//!
//! A client sends the bytes of an attachment as frames that come before the request naming them.
//! Whatever the request's outcome, they are taken out of the connection here, so a refused request
//! leaves nothing pending for the next one.
use crate::{
    agents::attachments::PreparedAttachment,
    avatars::AvatarPicture,
    core::Core,
    errors::{self, Result},
    writing::attachments::WritingImageAttachment,
};
use serde_json::Value;
use soyeht_protocol::method;

/// What `prepare_attachments` took out of the connection for one request.
#[derive(Default)]
pub(crate) struct Attachments {
    pub(crate) prompt: Vec<PreparedAttachment>,
    pub(crate) writing_image: Option<WritingImageAttachment>,
    pub(crate) avatar_picture: Option<AvatarPicture>,
}

/// Commits the attachments of `method` and rewrites `params` so that the request's identity names
/// their content. A method that takes none finds the connection with none pending.
pub(crate) fn prepare(
    core: &mut Core,
    connection: &str,
    method: &str,
    params: &mut Value,
) -> Result<Attachments> {
    let mut attachments = Attachments::default();
    match method {
        method::PANE_PROMPT => {
            attachments.prompt = core.prepare_prompt_attachments(connection, params)?;
        }
        method::WRITING_FORMAT => {
            attachments.writing_image = core.prepare_writing_image(connection, params)?;
        }
        method::AVATAR_CREATE => {
            attachments.avatar_picture = Some(crate::avatars::prepare_avatar_picture(
                core, connection, params,
            )?);
        }
        _ => {}
    }
    if attachments.writing_image.is_none()
        && attachments.avatar_picture.is_none()
        && core.web_attachment_pending(connection)?
    {
        return Err(errors::invalid(
            "Binary attachments must be committed by the following request",
        ));
    }
    Ok(attachments)
}
