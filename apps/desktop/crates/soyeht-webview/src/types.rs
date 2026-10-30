use std::{path::PathBuf, sync::Arc};

/// What a new view is made with.
#[derive(Debug, Clone)]
pub struct ViewOptions {
    /// Where the profile's cookies, local storage and cache live. `None` makes a private view
    /// that keeps nothing when it closes (an incognito pane).
    pub profile: Option<PathBuf>,
    /// Where a private view keeps what its page needs while it runs, in a folder of its own that
    /// the next start removes. It matters where the page cannot be kept in memory (Windows).
    pub scratch: Option<PathBuf>,
    /// The color of the page's face until it has drawn, as 0xRRGGBB.
    pub background: u32,
    /// The size the page lays out for, in logical pixels, until the pane's size is known.
    pub width: u32,
    pub height: u32,
}

/// The page's own facts, which change as it loads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageState {
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// 0.0 to 1.0.
    pub progress: f64,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// One picture of the page, as the pane shows it: blue, green, red and alpha bytes, rows of
/// `stride` bytes, with no transparency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The size of the picture in device pixels.
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub bytes: Vec<u8>,
    /// Device pixels in one logical pixel (1, or 2 on a high-density screen).
    pub scale: u32,
}

/// What a view tells the window, from the thread that serves it. Never carries pixels: the
/// window asks for the latest picture, so a window slower than the page skips pictures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewUpdate {
    /// A new picture is ready (`WebView::take_frame`).
    Frame,
    /// The page's state changed (`WebView::state`).
    State,
    /// A navigation began. `count` is the number of navigations this view has begun.
    Navigating { count: u64 },
    /// The person worked in the page itself: a page that is a window of its own takes their
    /// pointer and keys, which the program's window does not see.
    Person,
}

/// Where a view reports. It runs on the thread of the web view and must not block.
pub type ViewSink = Arc<dyn Fn(ViewUpdate) + Send + Sync>;

/// What to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation<'a> {
    Url(&'a str),
    Back,
    Forward,
    Reload,
    ReloadIgnoringCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Primary,
    Middle,
    Secondary,
}

/// Modifier keys held, as the window system names them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    Move { x: f64, y: f64 },
    Down { x: f64, y: f64, button: Button },
    Up { x: f64, y: f64, button: Button },
}

/// A scroll in lines, where positive `dy` moves the page up (the content scrolls down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollInput {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    pub keysym: crate::Keysym,
    pub pressed: bool,
    pub modifiers: Modifiers,
}

/// A window of the system, as the integer its handle is (an `HWND` on Windows). Zero is the
/// desktop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowHandle(pub isize);

/// A rectangle of the window's client area, in logical pixels from its top left.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where a page that is a window of the system's own sits over the window of the program, and
/// whether it shows. `page` is the pane's whole area, which the page lays out for; `clip` is the
/// part of it that is on screen (a pane scrolled half out of its stage shows half), so the page
/// never draws outside what the program shows of it. `scale` is the device pixels in one logical
/// pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub window: WindowHandle,
    pub page: Rect,
    pub clip: Rect,
    pub scale: f64,
    pub visible: bool,
}
