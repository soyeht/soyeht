//! The files the window draws that are not fonts: the line icons and the provider logos.
//!
//! The line icons are Lucide (ISC license, `assets/LICENSE-lucide.txt`), the closest open
//! equivalent of the Mac client's system symbols. The provider logos are the Mac client's own
//! resources, embedded from there.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

macro_rules! icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        /// A line icon.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($variant),*
        }

        impl Icon {
            pub fn path(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("icons/", $file, ".svg")),*
                }
            }
        }

        const ICONS: &[(&str, &[u8])] = &[
            $((
                concat!("icons/", $file, ".svg"),
                include_bytes!(concat!("../assets/icons/", $file, ".svg")),
            )),*
        ];
    };
}

icons! {
    Cpu => "cpu",
    FileText => "file-text",
    Globe => "globe",
    ListChecks => "list-checks",
    Terminal => "square-terminal",
    Smartphone => "smartphone",
    Feather => "feather",
    Copy => "copy",
    Clapperboard => "clapperboard",
    Settings => "settings",
    List => "list",
    Columns => "columns-2",
    Rows => "rows-2",
    Close => "x",
    Plus => "plus",
    Mic => "mic",
    ArrowUp => "arrow-up",
    ChevronDown => "chevron-down",
    Shield => "shield",
    Minimize => "minus",
    Maximize => "square",
    Grid => "layout-grid",
    ClipboardList => "clipboard-list",
    Circle => "circle",
    Check => "check",
    Refresh => "refresh-cw",
    Alert => "triangle-alert",
    Stopped => "circle-stop",
    ChevronRight => "chevron-right",
    ChevronLeft => "chevron-left",
    Clock => "clock",
    ClockAlert => "clock-alert",
    Paused => "circle-pause",
    QueueEnd => "list-end",
    Paperclip => "paperclip",
    Compass => "compass",
    Key => "key-round",
    ArrowRightCircle => "circle-arrow-right",
    Image => "image",
    File => "file",
    Audio => "audio-lines",
    Pdf => "file-type",
    Timer => "timer",
    Branch => "git-branch",
    Comment => "message-square",
    Send => "send",
    ChevronUp => "chevron-up",
    Hash => "hash",
    Calendar => "calendar",
    Gauge => "gauge",
    Palette => "palette",
    Skills => "wand-sparkles",
    PanelLeft => "panel-left",
    Folder => "folder",
    Eye => "eye",
    EyeOff => "eye-off",
    Search => "search",
    Bold => "bold",
    Italic => "italic",
    Heading => "heading",
    Quote => "text-quote",
    Play => "play",
    Info => "info",
}

macro_rules! logo {
    ($name:literal) => {
        include_bytes!(concat!(
            "../../../../shared/Sources/SoyehtClient/Resources/ProviderIcons.xcassets/",
            $name,
            ".imageset/",
            $name,
            ".png"
        ))
    };
}

/// The provider logos: the name a harness id contains, the path of the logo as a vector
/// graphic, and the PNG it wraps. The Mac client draws the PNG as a template image; wrapped in an
/// SVG it is a mask too, so a view tints it with a palette role as the Mac does.
const LOGOS: &[(&str, &str, &[u8])] = &[
    ("claude", "logos/claude.svg", logo!("claude")),
    ("codex", "logos/codex.svg", logo!("codex")),
    ("cursor", "logos/cursor.svg", logo!("cursor")),
    ("opencode", "logos/opencode.svg", logo!("opencode")),
];

/// The SVG that carries a logo's PNG.
fn logo_svg(png: &[u8]) -> Vec<u8> {
    let encoded = STANDARD.encode(png);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         viewBox=\"0 0 {LOGO_SIDE} {LOGO_SIDE}\" width=\"{LOGO_SIDE}\" height=\"{LOGO_SIDE}\">\
         <image width=\"{LOGO_SIDE}\" height=\"{LOGO_SIDE}\" xlink:href=\"data:image/png;base64,{encoded}\"/></svg>"
    )
    .into_bytes()
}

/// The side of the logo images, in pixels.
const LOGO_SIDE: u32 = 192;

/// The logo of the provider a harness id names ("registry.claude-acp" is Claude's).
pub fn logo_path(harness_id: &str) -> Option<&'static str> {
    let id = harness_id.to_lowercase();
    LOGOS
        .iter()
        .find(|(provider, _, _)| id.contains(provider))
        .map(|(_, path, _)| *path)
}

fn paths() -> impl Iterator<Item = &'static str> {
    ICONS
        .iter()
        .map(|(path, _)| *path)
        .chain(LOGOS.iter().map(|(_, path, _)| *path))
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ICONS.iter().find(|(known, _)| *known == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        Ok(LOGOS
            .iter()
            .find(|(_, known, _)| *known == path)
            .map(|(_, _, png)| Cow::Owned(logo_svg(png))))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(paths()
            .filter(|known| known.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg_the_source_can_load() {
        for (path, bytes) in ICONS {
            assert!(
                std::str::from_utf8(bytes).unwrap().contains("<svg"),
                "{path}"
            );
            assert!(Assets.load(path).unwrap().is_some());
        }
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
        assert_eq!(paths().count(), ICONS.len() + LOGOS.len());
    }

    #[test]
    fn a_harness_id_names_its_provider_logo() {
        assert_eq!(logo_path("registry.claude-acp"), Some("logos/claude.svg"));
        assert_eq!(logo_path("codex"), Some("logos/codex.svg"));
        assert_eq!(logo_path("registry.opencode"), Some("logos/opencode.svg"));
        assert_eq!(logo_path("custom-agent"), None);
    }

    #[test]
    fn a_logo_is_an_svg_that_carries_its_png() {
        for (_, path, png) in LOGOS {
            assert!(png.starts_with(b"\x89PNG"), "{path}");
            let svg = Assets.load(path).unwrap().unwrap();
            let text = std::str::from_utf8(&svg).unwrap();
            assert!(text.starts_with("<svg") && text.contains("data:image/png;base64,iVBOR"));
        }
    }
}
