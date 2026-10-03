//! `session/prompt` content. ACP v1 lets a client send image, audio and
//! embedded resource blocks only when the agent advertised them in
//! `promptCapabilities`; text and `resource_link` are the baseline every agent
//! accepts. Each attached file takes the richest form the agent supports and
//! the one-line message budget still fits, and otherwise travels as a link to
//! its absolute path.
use crate::Error;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{PromptAttachment, PromptAttachmentKind, limits};
use url::Url;

/// What the agent's `initialize` response allows beyond text and links.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCapabilities {
    pub image: bool,
    pub audio: bool,
    pub embedded_context: bool,
}

impl PromptCapabilities {
    /// A missing or malformed flag is false, as ACP v1 specifies.
    pub fn from_initialize(result: &Value) -> Self {
        let advertised = &result["agentCapabilities"]["promptCapabilities"];
        let flag = |name: &str| advertised[name].as_bool().unwrap_or(false);
        Self {
            image: flag("image"),
            audio: flag("audio"),
            embedded_context: flag("embeddedContext"),
        }
    }
}

/// Image types that claude-agent-acp and codex-acp forward as model input and
/// that both providers accept. Other images are linked.
const INLINE_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Room kept for the JSON-RPC envelope around `params`.
const ENVELOPE_BYTES: usize = 1024;

/// The ACP block a file is sent as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Image,
    Audio,
    Embedded,
    Link,
}

/// One attached file on the agent's host, described by the engine.
pub struct PromptFile<'a> {
    pub kind: &'a PromptAttachmentKind,
    pub name: &'a str,
    pub mime_type: &'a str,
    pub path: &'a str,
    pub size: u64,
}

impl<'a> PromptFile<'a> {
    /// Reads a queued attachment, whose engine-owned path is always set.
    pub fn from_attachment(attachment: &'a PromptAttachment) -> Result<Self, Error> {
        Ok(Self {
            kind: &attachment.kind,
            name: &attachment.name,
            mime_type: &attachment.mime_type,
            path: attachment
                .path
                .as_deref()
                .ok_or(Error::AttachmentPathMissing)?,
            size: attachment
                .bytes
                .parse()
                .map_err(|_| Error::AttachmentSizeInvalid)?,
        })
    }

    /// The richest block this agent accepts for the file, before the size
    /// budget is applied. A link needs no file contents.
    fn preferred(&self, capabilities: PromptCapabilities) -> Delivery {
        match self.kind {
            PromptAttachmentKind::Image
                if capabilities.image && INLINE_IMAGE_TYPES.contains(&self.mime_type) =>
            {
                Delivery::Image
            }
            PromptAttachmentKind::Audio
                if capabilities.audio && self.mime_type.starts_with("audio/") =>
            {
                Delivery::Audio
            }
            PromptAttachmentKind::Text if capabilities.embedded_context => Delivery::Embedded,
            _ => Delivery::Link,
        }
    }

    /// Whether the engine should read the file for this agent. Nothing larger
    /// than one ACP message can be inlined, so such files are never read.
    pub fn needs_contents(&self, capabilities: PromptCapabilities) -> bool {
        self.preferred(capabilities) != Delivery::Link
            && self.size <= limits::ACP_MESSAGE_BYTES as u64
    }

    fn uri(&self) -> Result<String, Error> {
        Url::from_file_path(self.path)
            .map(String::from)
            .map_err(|()| Error::AttachmentPathNotAbsolute)
    }

    fn link(&self) -> Result<Value, Error> {
        Ok(
            json!({"type":"resource_link","uri":self.uri()?,"name":self.name,
            "mimeType":self.mime_type,"size":self.size}),
        )
    }

    /// The inline block, or `None` when the contents cannot take that form,
    /// as text that is not UTF-8.
    fn inline(&self, delivery: Delivery, contents: &[u8]) -> Result<Option<Value>, Error> {
        Ok(match delivery {
            Delivery::Image => Some(
                json!({"type":"image","mimeType":self.mime_type,"data":STANDARD.encode(contents)}),
            ),
            Delivery::Audio => Some(
                json!({"type":"audio","mimeType":self.mime_type,"data":STANDARD.encode(contents)}),
            ),
            Delivery::Embedded => match std::str::from_utf8(contents) {
                Ok(text) => Some(json!({"type":"resource","resource":{"uri":self.uri()?,
                    "mimeType":self.mime_type,"text":text}})),
                Err(_) => None,
            },
            Delivery::Link => None,
        })
    }
}

/// Builds `session/prompt` parameters: the text first, then one block per file
/// in order. `contents` holds the bytes the engine read for files whose
/// `needs_contents` was true. Files are inlined in order while the encoded
/// message stays within one ACP line; the rest are linked.
pub fn prompt_with_files(
    session_id: &str,
    text: &str,
    files: &[(PromptFile<'_>, Option<&[u8]>)],
    capabilities: PromptCapabilities,
) -> Result<Value, Error> {
    let mut blocks = vec![json!({"type":"text","text":text})];
    for (file, _) in files {
        blocks.push(file.link()?);
    }
    let linked = json!({"sessionId":session_id,"prompt":blocks});
    let mut remaining = limits::ACP_MESSAGE_BYTES
        .saturating_sub(serde_json::to_vec(&linked)?.len() + ENVELOPE_BYTES);
    for (index, (file, contents)) in files.iter().enumerate() {
        let delivery = file.preferred(capabilities);
        let Some(contents) = contents.filter(|_| delivery != Delivery::Link) else {
            continue;
        };
        let Some(block) = file.inline(delivery, contents)? else {
            continue;
        };
        let added = serde_json::to_vec(&block)?.len();
        let removed = serde_json::to_vec(&blocks[index + 1])?.len();
        let Some(left) = remaining.checked_sub(added.saturating_sub(removed)) else {
            continue;
        };
        remaining = left;
        blocks[index + 1] = block;
    }
    Ok(json!({"sessionId":session_id,"prompt":blocks}))
}

/// Describes a non-text block of a replayed user message, so the restored
/// transcript shows what was attached. Unknown blocks are skipped.
pub fn replayed_attachment(content: &Value) -> Option<PromptAttachment> {
    let text = |key: &str| content[key].as_str().filter(|value| !value.is_empty());
    let local_path = |uri: &str| {
        Url::parse(uri)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .and_then(|path| path.to_str().map(str::to_owned))
    };
    let attachment =
        |kind, name: &str, mime: &str, bytes: u64, path: Option<String>| PromptAttachment {
            kind,
            name: name.chars().take(256).collect(),
            mime_type: mime.chars().take(255).collect(),
            bytes: bytes.to_string(),
            attachment_id: None,
            data: None,
            path,
            thumbnail: None,
        };
    match content["type"].as_str()? {
        kind @ ("image" | "audio") => {
            let mime = text("mimeType")?;
            let bytes = STANDARD.decode(text("data")?).ok()?.len() as u64;
            let (kind, name) = if kind == "image" {
                (PromptAttachmentKind::Image, "Image")
            } else {
                (PromptAttachmentKind::Audio, "Audio")
            };
            Some(attachment(kind, name, mime, bytes, None))
        }
        "resource_link" => {
            let kind = if text("mimeType").is_some_and(|mime| mime.starts_with("text/")) {
                PromptAttachmentKind::Text
            } else {
                PromptAttachmentKind::File
            };
            Some(attachment(
                kind,
                text("name")?,
                text("mimeType").unwrap_or("application/octet-stream"),
                content["size"].as_u64().unwrap_or(0),
                local_path(text("uri")?),
            ))
        }
        "resource" => {
            let resource = &content["resource"];
            let uri = resource["uri"].as_str()?;
            let path = local_path(uri);
            let name = path
                .as_deref()
                .and_then(|path| path.rsplit('/').next())
                .unwrap_or(uri)
                .to_owned();
            let size = resource["text"]
                .as_str()
                .map(str::len)
                .or_else(|| resource["blob"].as_str().map(|blob| blob.len() / 4 * 3))?;
            Some(attachment(
                PromptAttachmentKind::Text,
                &name,
                resource["mimeType"].as_str().unwrap_or("text/plain"),
                size as u64,
                path,
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(kind: &'a PromptAttachmentKind, mime: &'a str, path: &'a str) -> PromptFile<'a> {
        PromptFile {
            kind,
            name: path.rsplit('/').next().unwrap(),
            mime_type: mime,
            path,
            size: 5,
        }
    }

    /// The type of each block after the text.
    fn types(value: &Value) -> Vec<&str> {
        value["prompt"].as_array().unwrap()[1..]
            .iter()
            .map(|block| block["type"].as_str().unwrap())
            .collect()
    }

    const ALL: PromptCapabilities = PromptCapabilities {
        image: true,
        audio: true,
        embedded_context: true,
    };

    #[test]
    fn capabilities_follow_the_initialize_response_and_default_to_false() {
        let codex = json!({"agentCapabilities":{"promptCapabilities":{
            "embeddedContext":true,"image":true}}});
        assert_eq!(
            PromptCapabilities::from_initialize(&codex),
            PromptCapabilities {
                image: true,
                audio: false,
                embedded_context: true
            }
        );
        assert_eq!(
            PromptCapabilities::from_initialize(&json!({"agentCapabilities":{}})),
            PromptCapabilities::default()
        );
        let malformed = json!({"agentCapabilities":{"promptCapabilities":{"image":"yes"}}});
        assert!(!PromptCapabilities::from_initialize(&malformed).image);
    }

    #[test]
    fn each_file_takes_the_richest_block_the_agent_advertised() {
        let image = PromptAttachmentKind::Image;
        let audio = PromptAttachmentKind::Audio;
        let text = PromptAttachmentKind::Text;
        let other = PromptAttachmentKind::File;
        let files = [
            (
                file(&image, "image/png", "/tmp/a b.png"),
                Some(&b"PNG!!"[..]),
            ),
            (
                file(&audio, "audio/wav", "/tmp/voice.wav"),
                Some(&b"RIFF!"[..]),
            ),
            (
                file(&text, "text/markdown", "/tmp/notes.md"),
                Some(&b"# Hi\n"[..]),
            ),
            (file(&other, "application/pdf", "/tmp/report.pdf"), None),
        ];
        let value = prompt_with_files("s", "Look", &files, ALL).unwrap();
        let blocks = value["prompt"].as_array().unwrap();
        assert_eq!(blocks[0], json!({"type":"text","text":"Look"}));
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["data"], STANDARD.encode(b"PNG!!"));
        assert_eq!(blocks[2]["type"], "audio");
        assert_eq!(blocks[3]["type"], "resource");
        assert_eq!(blocks[3]["resource"]["text"], "# Hi\n");
        assert_eq!(blocks[3]["resource"]["uri"], "file:///tmp/notes.md");
        assert_eq!(
            blocks[4],
            json!({"type":"resource_link","uri":"file:///tmp/report.pdf","name":"report.pdf",
                "mimeType":"application/pdf","size":5})
        );
        assert_eq!(
            types(&value),
            ["image", "audio", "resource", "resource_link"]
        );

        let linked = prompt_with_files("s", "Look", &files, PromptCapabilities::default()).unwrap();
        assert_eq!(types(&linked), ["resource_link"; 4]);
        assert_eq!(linked["prompt"][1]["uri"], "file:///tmp/a%20b.png");
        assert_eq!(linked["prompt"][1]["type"], "resource_link");
    }

    #[test]
    fn unsupported_images_binary_text_and_missing_contents_are_linked() {
        let image = PromptAttachmentKind::Image;
        let text = PromptAttachmentKind::Text;
        let heic = file(&image, "image/heic", "/tmp/photo.heic");
        assert_eq!(heic.preferred(ALL), Delivery::Link);
        assert!(!heic.needs_contents(ALL));
        let files = [
            (heic, Some(&b"heic"[..])),
            (
                file(&text, "text/plain", "/tmp/binary.txt"),
                Some(&[0xff, 0xfe][..]),
            ),
            (file(&image, "image/png", "/tmp/unread.png"), None),
        ];
        let value = prompt_with_files("s", "Look", &files, ALL).unwrap();
        assert_eq!(types(&value), ["resource_link"; 3]);
        let relative = PromptFile {
            path: "relative.png",
            ..file(&image, "image/png", "/tmp/x.png")
        };
        assert!(prompt_with_files("s", "Look", &[(relative, None)], ALL).is_err());
    }

    #[test]
    fn inlining_stops_at_the_one_message_budget_and_links_the_rest() {
        let image = PromptAttachmentKind::Image;
        let large = vec![7u8; limits::ACP_MESSAGE_BYTES / 2];
        let mut big = file(&image, "image/png", "/tmp/one.png");
        big.size = large.len() as u64;
        let mut second = file(&image, "image/png", "/tmp/two.png");
        second.size = large.len() as u64;
        assert!(big.needs_contents(ALL));
        let files = [
            (big, Some(large.as_slice())),
            (second, Some(large.as_slice())),
        ];
        let value = prompt_with_files("s", "Look", &files, ALL).unwrap();
        assert_eq!(types(&value), ["image", "resource_link"]);
        let message = crate::Message::Request {
            id: json!("s-18446744073709551615"),
            method: "session/prompt".into(),
            params: value,
        };
        assert!(message.encode().is_ok(), "the prompt fits one ACP line");
    }

    #[test]
    fn replayed_blocks_describe_their_attachments() {
        let image = replayed_attachment(&json!({"type":"image","mimeType":"image/png",
            "data":STANDARD.encode(b"12345")}))
        .unwrap();
        assert_eq!(image.kind, PromptAttachmentKind::Image);
        assert_eq!(image.bytes, "5");
        let link = replayed_attachment(&json!({"type":"resource_link",
            "uri":"file:///tmp/a%20b.pdf","name":"a b.pdf","mimeType":"application/pdf",
            "size":9}))
        .unwrap();
        assert_eq!(link.path.as_deref(), Some("/tmp/a b.pdf"));
        assert_eq!(link.kind, PromptAttachmentKind::File);
        let embedded = replayed_attachment(&json!({"type":"resource","resource":{
            "uri":"file:///tmp/notes.md","text":"# Hi"}}))
        .unwrap();
        assert_eq!(embedded.name, "notes.md");
        assert_eq!(embedded.bytes, "4");
        assert!(replayed_attachment(&json!({"type":"text","text":"hi"})).is_none());
    }
}
