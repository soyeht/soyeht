//! Files attached in the composer and sent with a prompt: what each one is, how it reads on its
//! chip, how many may go with a message, and how the engine is told about it. The engine decides
//! how a file reaches the agent; the client describes it and delivers its bytes.
//!
//! The rules and the wording are the Mac's (`AgentAttachmentDraft`, `AgentAttachmentList`,
//! `AgentAttachmentPresentation`, `AgentAttachmentStaging`, `AgentPasteboardContent`); both
//! clients pass the cases of `contracts/vectors/attachments.json`.

mod image_prep;
mod kinds;
mod size;
mod stage;
#[cfg(test)]
mod vectors;

pub use image_prep::{
    IMAGE_FALLBACK_NAME, IMAGE_MAXIMUM_BYTES, IMAGE_MAXIMUM_PIXELS, image_file_name, keeps_original,
};
pub use kinds::{classify, kind_of_file, mime_for};
pub use size::file_size;
pub use stage::{stage_bytes, stage_image, stage_path};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_protocol::{
    AttachmentRef, PromptAttachment, PromptAttachmentKind,
    limits::{PROMPT_ATTACHMENT_BYTES, PROMPT_ATTACHMENT_TOTAL_BYTES, PROMPT_ATTACHMENTS},
};
use uuid::Uuid;

/// Where an attachment's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Bytes sent through the engine as binary frames.
    Bytes(Vec<u8>),
    /// A file already on the engine's machine, linked where it is and never copied.
    HostFile(String),
}

/// A file attached in the composer and not yet sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub id: String,
    pub kind: PromptAttachmentKind,
    pub name: String,
    pub mime_type: String,
    pub byte_count: usize,
    pub source: Source,
    /// A small PNG or JPEG preview kept with the transcript.
    pub thumbnail: Option<Vec<u8>>,
}

/// The bytes of an attachment, to send as binary frames before the request that names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Upload {
    pub reference: AttachmentRef,
    pub bytes: Vec<u8>,
}

impl Draft {
    pub fn new(
        kind: PromptAttachmentKind,
        name: String,
        mime_type: String,
        byte_count: usize,
        source: Source,
        thumbnail: Option<Vec<u8>>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind,
            name,
            mime_type,
            byte_count,
            source,
            thumbnail,
        }
    }

    /// Bytes this draft sends through the engine; a linked file sends none.
    fn uploaded_bytes(&self) -> usize {
        match &self.source {
            Source::Bytes(bytes) => bytes.len(),
            Source::HostFile(_) => 0,
        }
    }

    /// The protocol's description of it and, for bytes, what carries them.
    pub fn wire(&self) -> (PromptAttachment, Option<Upload>) {
        let thumbnail = self
            .thumbnail
            .as_deref()
            .map(|bytes| STANDARD.encode(bytes));
        let mut attachment = PromptAttachment {
            kind: self.kind.clone(),
            name: self.name.clone(),
            mime_type: self.mime_type.clone(),
            bytes: self.byte_count.to_string(),
            attachment_id: None,
            data: None,
            path: None,
            thumbnail,
        };
        match &self.source {
            Source::Bytes(bytes) => {
                let id = self.id.to_lowercase();
                attachment.bytes = bytes.len().to_string();
                attachment.attachment_id = Some(id.clone());
                let reference = AttachmentRef {
                    attachment_id: id,
                    mime_type: self.mime_type.clone(),
                    name: self.name.clone(),
                    bytes: bytes.len().to_string(),
                };
                (
                    attachment,
                    Some(Upload {
                        reference,
                        bytes: bytes.clone(),
                    }),
                )
            }
            Source::HostFile(path) => {
                attachment.path = Some(path.clone());
                (attachment, None)
            }
        }
    }
}

/// Why the composer refused a file. The engine enforces the same limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    TooMany,
    TooLarge { name: String },
    TotalTooLarge,
    Empty { name: String },
    Unreadable { name: String },
    Folder { name: String },
}

impl Rejection {
    pub fn message(&self) -> String {
        match self {
            Self::TooMany => format!("A message takes at most {PROMPT_ATTACHMENTS} attachments."),
            Self::TooLarge { name } => {
                format!(
                    "{name} is larger than {} MB.",
                    mebibytes(PROMPT_ATTACHMENT_BYTES)
                )
            }
            Self::TotalTooLarge => format!(
                "Attachments are limited to {} MB per message.",
                mebibytes(PROMPT_ATTACHMENT_TOTAL_BYTES)
            ),
            Self::Empty { name } => format!("{name} is empty."),
            Self::Unreadable { name } => format!("{name} could not be read."),
            Self::Folder { name } => format!("{name} is a folder; attach the files inside it."),
        }
    }
}

fn mebibytes(bytes: usize) -> usize {
    bytes / (1_024 * 1_024)
}

/// The attachments of the next prompt, kept within the engine's limits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct List {
    items: Vec<Draft>,
}

impl List {
    pub fn items(&self) -> &[Draft] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn add(&mut self, draft: Draft) -> Result<(), Rejection> {
        if self.items.len() >= PROMPT_ATTACHMENTS {
            return Err(Rejection::TooMany);
        }
        if matches!(&draft.source, Source::Bytes(bytes) if bytes.is_empty()) {
            return Err(Rejection::Empty { name: draft.name });
        }
        if draft.uploaded_bytes() > PROMPT_ATTACHMENT_BYTES {
            return Err(Rejection::TooLarge { name: draft.name });
        }
        let total = self
            .items
            .iter()
            .fold(draft.uploaded_bytes(), |total, held| {
                total + held.uploaded_bytes()
            });
        if total > PROMPT_ATTACHMENT_TOTAL_BYTES {
            return Err(Rejection::TotalTooLarge);
        }
        self.items.push(draft);
        Ok(())
    }

    pub fn remove(&mut self, id: &str) {
        self.items.retain(|draft| draft.id != id);
    }

    /// Hands the drafts to the prompt being sent and empties the list.
    pub fn take(&mut self) -> Vec<Draft> {
        std::mem::take(&mut self.items)
    }
}

/// The composer's attachments for its next prompt, with the latest refusal: a refused file leaves
/// a notice instead of a chip.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Staged {
    list: List,
    notice: Option<String>,
}

impl Staged {
    pub fn items(&self) -> &[Draft] {
        self.list.items()
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Whether the composer shows anything of this: a chip or a notice.
    pub fn is_shown(&self) -> bool {
        !self.list.is_empty() || self.notice.is_some()
    }

    pub fn add(&mut self, results: Vec<Result<Draft, Rejection>>) {
        self.notice = None;
        for result in results {
            if let Err(rejection) = result.and_then(|draft| self.list.add(draft)) {
                self.notice = Some(rejection.message());
            }
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.list.remove(id);
        self.notice = None;
    }

    /// Hands the drafts to the prompt being sent and empties the composer.
    pub fn take(&mut self) -> Vec<Draft> {
        self.notice = None;
        self.list.take()
    }
}

/// The picture on a chip that has no thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Image,
    Audio,
    Text,
    Pdf,
    File,
}

impl Glyph {
    pub fn of(kind: &PromptAttachmentKind, mime_type: &str) -> Self {
        match kind {
            PromptAttachmentKind::Image => Self::Image,
            PromptAttachmentKind::Audio => Self::Audio,
            PromptAttachmentKind::Text => Self::Text,
            PromptAttachmentKind::File if mime_type == "application/pdf" => Self::Pdf,
            PromptAttachmentKind::File => Self::File,
        }
    }
}

/// "PNG · 12 KB": the subtype of the MIME type and a file-style size.
pub fn detail(mime_type: &str, bytes: usize) -> String {
    let subtype = mime_type.rsplit('/').next().unwrap_or(mime_type);
    let format = subtype.split('+').next().unwrap_or_default().to_uppercase();
    if bytes > 0 {
        format!("{format} \u{b7} {}", file_size(bytes))
    } else {
        format
    }
}

/// How one attachment chip reads, in the composer and in a sent message.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub id: String,
    pub name: String,
    pub glyph: Glyph,
    pub detail: String,
    pub thumbnail: Option<Vec<u8>>,
}

impl Chip {
    pub fn of_draft(draft: &Draft) -> Self {
        Self {
            id: draft.id.clone(),
            name: draft.name.clone(),
            glyph: Glyph::of(&draft.kind, &draft.mime_type),
            detail: detail(&draft.mime_type, draft.byte_count),
            thumbnail: draft.thumbnail.clone(),
        }
    }

    /// A sent attachment: the transcript keeps its name, size and thumbnail.
    pub fn of_sent(attachment: &PromptAttachment, index: usize) -> Self {
        Self {
            id: format!(
                "{index}-{}",
                attachment.path.as_deref().unwrap_or(&attachment.name)
            ),
            name: attachment.name.clone(),
            glyph: Glyph::of(&attachment.kind, &attachment.mime_type),
            detail: detail(&attachment.mime_type, attachment.bytes.parse().unwrap_or(0)),
            thumbnail: attachment.thumbnail.as_deref().and_then(decode_base64),
        }
    }
}

/// The bytes a base64 text stands for; none when it is not base64, so that a thumbnail the
/// engine kept damaged is a chip without a picture.
#[allow(
    clippy::manual_ok_err,
    reason = "text that is not base64 has no bytes, and that is the answer"
)]
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    match STANDARD.decode(text) {
        Ok(bytes) => Some(bytes),
        Err(_) => None,
    }
}

/// What a paste offers the composer. Copied files and copied images become attachments;
/// anything else is left for the text field to paste as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteOffer {
    pub files: usize,
    pub image: bool,
    pub text: bool,
}

impl PasteOffer {
    /// Copied files always attach: a file manager also puts their names on the clipboard as text.
    /// An image attaches unless the copy is text that carries a picture of itself, as a
    /// spreadsheet cell does.
    pub fn offers_attachments(&self) -> bool {
        self.files > 0 || (self.image && !self.text)
    }
}
