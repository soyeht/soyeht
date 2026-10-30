//! Systems with no system web view of ours: every call says so.

use crate::{
    Error, Frame, KeyInput, Modifiers, Navigation, PageState, Placement, Pointer, Result,
    ScrollInput, ViewOptions, ViewSink,
};

pub struct Backend;

impl Backend {
    pub fn get() -> Result<&'static Self> {
        Err(Error::Unsupported)
    }

    pub fn open(&'static self, _: &ViewOptions, _: ViewSink) -> Result<WebView> {
        Err(Error::Unsupported)
    }
}

/// Never made on this system: `Backend::open` does not make one.
pub struct WebView;

impl WebView {
    pub fn navigate(&self, _: Navigation<'_>) -> Result<()> {
        Err(Error::Unsupported)
    }
    pub fn resize(&self, _: u32, _: u32) {}
    pub fn place(&self, _: Placement) {}
    pub fn pointer(&self, _: Pointer, _: Modifiers) {}
    pub fn scroll(&self, _: ScrollInput, _: Modifiers) {}
    pub fn key(&self, _: KeyInput) {}
    pub fn focus(&self, _: bool) {}
    pub fn insert_text(&self, _: &str) {}
    pub fn select_all(&self) {}
    pub fn delete_selection(&self) {}
    pub fn take_frame(&self) -> Option<Frame> {
        None
    }
    pub fn state(&self) -> PageState {
        PageState::default()
    }
    pub fn navigations(&self) -> u64 {
        0
    }
    pub fn capture(&self) -> Result<Frame> {
        Err(Error::Unsupported)
    }
    pub fn run_script(&self, _: &str) -> Result<String> {
        Err(Error::Unsupported)
    }
    pub fn cookies_present(&self, _: &str) -> Result<bool> {
        Err(Error::Unsupported)
    }
}
