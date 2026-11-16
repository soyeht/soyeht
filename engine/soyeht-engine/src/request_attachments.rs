//! The binary attachments a request carried, committed before its stored result is looked up.
//!
//! A client sends the bytes of an attachment as frames that come before the request naming them.
//! Once a request reaches `prepare` the frames are taken out of the connection whatever comes after
//! (a request that is then refused or repeated leaves nothing pending). Before it, and after it,
//! `settle` decides what a request leaves behind; the rule is the same for every method.
use crate::{
    agents::attachments::PreparedAttachment,
    avatars::AvatarPicture,
    core::Core,
    errors::{self, Result},
    writing::attachments::WritingImageAttachment,
};
use serde_json::Value;
use soyeht_protocol::{method, method_family};

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

/// Drops the frames a request leaves pending on its connection, when they are its own.
///
/// - A request that takes attachments (`method_family::ATTACHMENTS`, which the contract derives) and
///   was refused before it committed them, by the schema or by the role of the caller, drops them:
///   nobody else will, and every later request would fail with "must be committed".
/// - `hello` and the pairing requests take none, so frames pending when they arrive are strays and
///   are dropped.
/// - Any other request that finds frames pending is refused by `prepare` and leaves them as they
///   are: they belong to the request still on its way, and an unrelated request must not destroy
///   an upload it interrupted.
pub(crate) fn settle(core: &mut Core, connection: &str, method: Option<&str>, refused: bool) {
    let Some(method) = method else { return };
    let strays = method == method::HELLO || method_family::PAIR.contains(&method);
    let owned = refused && method_family::ATTACHMENTS.contains(&method);
    if (strays || owned)
        && let Some(client) = core.clients.get_mut(connection)
    {
        client.attachments.discard();
    }
}
