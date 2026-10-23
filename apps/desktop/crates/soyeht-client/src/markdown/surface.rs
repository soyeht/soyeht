//! What a click on a link does where Markdown is drawn, as the Mac's `MarkdownSurface` does (the
//! cases are `contracts/vectors/editor.json`, `markdownPreview.surfaces`, which both clients
//! pass).
//!
//! A file's content is not trusted to open anything (ADR 0053 and 0087): in the editor's preview
//! an address keeps the look of a link and opens nothing, and a file reference is plain text. In
//! an agent reply an address opens in the browser and a reference opens in the editor.

/// Where a run of Markdown is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// An agent's reply.
    Reply,
    /// The editor's preview of a file.
    Preview,
}

impl Surface {
    /// Whether a click on a written address opens it.
    pub fn opens_addresses(self) -> bool {
        self == Self::Reply
    }

    /// Whether a file reference is a link that opens its file.
    pub fn opens_files(self) -> bool {
        self == Self::Reply
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{text, vector};

    #[test]
    fn each_surface_opens_what_the_vector_says() {
        let surfaces = vector("editor")["markdownPreview"]["surfaces"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(surfaces.len(), 2);
        for case in surfaces {
            let surface = match text(&case["surface"]) {
                "reply" => Surface::Reply,
                "preview" => Surface::Preview,
                other => panic!("unknown surface {other}"),
            };
            let name = text(&case["name"]);
            assert_eq!(
                surface.opens_addresses(),
                case["opensAddresses"].as_bool().unwrap(),
                "{name}"
            );
            assert_eq!(
                surface.opens_files(),
                case["opensFiles"].as_bool().unwrap(),
                "{name}"
            );
        }
    }
}
