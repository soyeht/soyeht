//! What a file is, from its name: the kind the engine sorts it by and its MIME type. The Mac asks
//! the system's type database; the desktop client has none, so this table holds the types people
//! attach, with the answers the Mac gives (`contracts/vectors/attachments.json` holds the cases
//! both must agree on). A name the table does not know is an opaque file.

use soyeht_protocol::PromptAttachmentKind::{self, Audio, File, Image, Text};

/// The MIME type of a text file the system has no type for.
const PLAIN_TEXT: &str = "text/plain";
/// The MIME type of a file nothing is known about.
const OPAQUE: &str = "application/octet-stream";

/// Extensions (lowercase), the kind, and the MIME type; none for a type the Mac names no MIME for.
const TABLE: &[(&[&str], PromptAttachmentKind, Option<&str>)] = &[
    (&["png"], Image, Some("image/png")),
    (&["jpg", "jpeg"], Image, Some("image/jpeg")),
    (&["gif"], Image, Some("image/gif")),
    (&["webp"], Image, Some("image/webp")),
    (&["heic"], Image, Some("image/heic")),
    (&["heif"], Image, Some("image/heif")),
    (&["tif", "tiff"], Image, Some("image/tiff")),
    (&["bmp"], Image, Some("image/bmp")),
    (&["svg"], Image, Some("image/svg+xml")),
    (&["avif"], Image, Some("image/avif")),
    (&["mp3"], Audio, Some("audio/mpeg")),
    (&["wav"], Audio, Some("audio/vnd.wave")),
    (&["m4a"], Audio, Some("audio/x-m4a")),
    (&["aac"], Audio, Some("audio/aac")),
    (&["flac"], Audio, Some("audio/flac")),
    (&["aif", "aiff"], Audio, Some("audio/aiff")),
    (&["txt"], Text, Some("text/plain")),
    (&["md", "markdown"], Text, Some("text/markdown")),
    (&["json"], Text, Some("application/json")),
    (&["csv"], Text, Some("text/csv")),
    (&["tsv"], Text, Some("text/tab-separated-values")),
    (&["html", "htm"], Text, Some("text/html")),
    (&["xml"], Text, Some("application/xml")),
    (&["yaml", "yml"], Text, Some("application/x-yaml")),
    (&["js"], Text, Some("text/javascript")),
    (&["css"], Text, Some("text/css")),
    (
        &["swift", "toml", "log", "sh", "c", "h", "cpp", "hpp", "java"],
        Text,
        None,
    ),
    (&["pdf"], File, Some("application/pdf")),
    (&["zip"], File, Some("application/zip")),
    (&["tar"], File, Some("application/x-tar")),
    (&["gz"], File, Some("application/x-gzip")),
    (&["mp4"], File, Some("video/mp4")),
    (&["mov"], File, Some("video/quicktime")),
    (
        &["docx"],
        File,
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ),
    (
        &["xlsx"],
        File,
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
    ),
    (
        &["pptx"],
        File,
        Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
    ),
];

/// The kind and the MIME type of a file named `name`.
pub fn classify(name: &str) -> (PromptAttachmentKind, String) {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_lowercase())
        .unwrap_or_default();
    TABLE
        .iter()
        .find(|(extensions, _, _)| extensions.contains(&extension.as_str()))
        .map_or_else(
            || (File, OPAQUE.to_owned()),
            |(_, kind, mime)| {
                let fallback = if *kind == Text { PLAIN_TEXT } else { OPAQUE };
                (kind.clone(), mime.unwrap_or(fallback).to_owned())
            },
        )
}

/// The kind of a file named `name`.
pub fn kind_of_file(name: &str) -> PromptAttachmentKind {
    classify(name).0
}

/// The MIME type of a file named `name`.
pub fn mime_for(name: &str) -> String {
    classify(name).1
}
