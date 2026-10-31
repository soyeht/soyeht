//! The page of a web pane on the window. Where the system's page is drawn off screen it is the
//! picture the system web view draws, and the person's pointer, wheel and keys are sent back to it,
//! so the page sees them as events of a real device (ADR 0110). Where the page is a window of its
//! own (Windows) the surface only says where the pane is, and the window of the page takes the
//! person's input itself (ADR 0124).

use crate::{composer_field::edit_of, theme::Palette, web_page::WebPage};
use gpui::{
    App, AppContext, Bounds, ClipboardItem, ContentMask, Context, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent,
    KeyUpEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    NavigationDirection, ParentElement, Pixels, Point, Render, RenderImage, ScrollDelta,
    ScrollWheelEvent, Styled, UTF16Selection, Window, canvas, div, point, px, rgb, size,
};
use soyeht_client::text_input::keys::Edit;
use soyeht_webview::{
    Button, Frame, KeyInput, Modifiers, Navigation, Placement, Pointer, Rect, ScrollInput,
    WindowHandle, keysym_for_key,
};
use std::{ops::Range, sync::Arc};

/// A wheel step is this many pixels of page, as a browser scrolls a line.
const LINE_PIXELS: f32 = 40.0;
/// What the page reads, once, for the clipboard.
const SELECTION: &str = "String(window.getSelection())";

pub(crate) struct WebSurface {
    page: Arc<WebPage>,
    focus: FocusHandle,
    palette: &'static Palette,
    /// Where the page was last painted.
    bounds: Option<Bounds<Pixels>>,
    /// The size the page was last told to lay out for, in logical pixels.
    sent_size: (u32, u32),
    /// The picture on screen, and its device pixels for each logical pixel.
    picture: Option<(Arc<RenderImage>, u32)>,
    /// Whether the page was last told that it has the keyboard.
    told_focus: bool,
    /// A button went down on the page and has not come up: its release is the page's, wherever
    /// the pointer is when it comes.
    pressed: bool,
    /// Something of the window is drawn over the page (a menu, a popover): a page that is a
    /// window of its own would be above it, so it is taken off the screen meanwhile.
    covered: bool,
}

impl Focusable for WebSurface {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl WebSurface {
    pub(crate) fn new(
        page: Arc<WebPage>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            page,
            focus: cx.focus_handle(),
            palette,
            bounds: None,
            sent_size: (0, 0),
            picture: None,
            told_focus: false,
            pressed: false,
            covered: false,
        }
    }

    pub(crate) fn style(&mut self, palette: &'static Palette) {
        self.palette = palette;
    }

    /// Whether something of the window is over the page.
    pub(crate) fn cover(&mut self, covered: bool) {
        self.covered = covered;
    }

    /// Where a window position is on the page, in logical pixels.
    fn on_page(&self, position: Point<Pixels>) -> (f64, f64) {
        let origin = self
            .bounds
            .map_or(point(px(0.0), px(0.0)), |bounds| bounds.origin);
        (
            f64::from(f32::from(position.x - origin.x)),
            f64::from(f32::from(position.y - origin.y)),
        )
    }

    fn modifiers(modifiers: &gpui::Modifiers) -> Modifiers {
        Modifiers {
            shift: modifiers.shift,
            control: modifiers.control,
            alt: modifiers.alt,
            logo: modifiers.platform,
        }
    }

    fn mouse_down(&mut self, down: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.page.control().note_person();
        let (x, y) = self.on_page(down.position);
        let view = self.page.view();
        let modifiers = Self::modifiers(&down.modifiers);
        match down.button {
            MouseButton::Navigate(direction) => {
                let step = match direction {
                    NavigationDirection::Back => Navigation::Back,
                    NavigationDirection::Forward => Navigation::Forward,
                };
                if view.navigate(step).is_err() {
                    // Back and forward are always valid requests; there is nothing to tell.
                }
            }
            button => {
                if let Some(button) = button_of(button) {
                    self.pressed = true;
                    view.pointer(Pointer::Down { x, y, button }, modifiers);
                }
            }
        }
        cx.stop_propagation();
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.release(event);
        cx.stop_propagation();
    }

    /// A button released outside the page, which went down on it: the page learns of it, and the
    /// release still belongs to whatever is under the pointer.
    fn mouse_up_out(&mut self, event: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        if self.pressed {
            self.release(event);
        }
    }

    fn release(&mut self, event: &MouseUpEvent) {
        self.pressed = false;
        self.page.control().note_person();
        let (x, y) = self.on_page(event.position);
        if let Some(button) = button_of(event.button) {
            self.page.view().pointer(
                Pointer::Up { x, y, button },
                Self::modifiers(&event.modifiers),
            );
        }
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, _: &mut Context<Self>) {
        let (x, y) = self.on_page(event.position);
        self.page
            .view()
            .pointer(Pointer::Move { x, y }, Self::modifiers(&event.modifiers));
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.page.control().note_person();
        let (x, y) = self.on_page(event.position);
        let pixels = match event.delta {
            ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
            ScrollDelta::Lines(lines) => (lines.x * LINE_PIXELS, lines.y * LINE_PIXELS),
        };
        // The wheel moves the page the way the content goes: a positive step of the window is the
        // page moving down, which is the web view's negative one.
        let (dx, dy) = (
            f64::from(-pixels.0 / LINE_PIXELS),
            f64::from(-pixels.1 / LINE_PIXELS),
        );
        self.page.view().scroll(
            ScrollInput { x, y, dx, dy },
            Self::modifiers(&event.modifiers),
        );
        cx.stop_propagation();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.edit(keystroke, cx) {
            cx.stop_propagation();
            return;
        }
        if self.key(keystroke, true) {
            cx.stop_propagation();
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.key(&event.keystroke, false) {
            cx.stop_propagation();
        }
    }

    /// A key the page reads as a key of a keyboard. False when the key has no name for the page.
    fn key(&self, keystroke: &Keystroke, pressed: bool) -> bool {
        self.page.control().note_person();
        let Some(keysym) = keysym_for_key(&keystroke.key, keystroke.key_char.as_deref()) else {
            return false;
        };
        self.page.view().key(KeyInput {
            keysym,
            pressed,
            modifiers: Self::modifiers(&keystroke.modifiers),
        });
        true
    }

    /// Copy, cut, paste and select all, which belong to the window's clipboard: the engine's own
    /// has no window of this program to ask on a Wayland session.
    fn edit(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        match edit_of(keystroke) {
            Some(Edit::Copy) => self.copy(false, cx),
            Some(Edit::Cut) => self.copy(true, cx),
            Some(Edit::Paste) => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.page.view().insert_text(&text);
                }
            }
            Some(Edit::SelectAll) => self.page.view().select_all(),
            _ => return false,
        }
        true
    }

    /// Puts what is selected on the page on the clipboard, and removes it when `cut`.
    fn copy(&mut self, cut: bool, cx: &mut Context<Self>) {
        let page = Arc::clone(&self.page);
        cx.spawn(async move |_, cx| {
            let reading = Arc::clone(&page);
            let text = cx
                .background_spawn(async move { reading.view().run_script(SELECTION) })
                .await;
            if let Ok(text) = text
                && !text.is_empty()
            {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
                if cut {
                    page.view().delete_selection();
                }
            }
        })
        .detach();
    }

    /// Paints the page into `bounds`, and brings the page up to date with the window: its size,
    /// the keyboard, and the latest picture.
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.bounds = Some(bounds);
        if soyeht_webview::embedded() {
            self.place(bounds, window, cx);
            return;
        }
        let wanted = (
            f32::from(bounds.size.width).floor().max(1.0) as u32,
            f32::from(bounds.size.height).floor().max(1.0) as u32,
        );
        if wanted != self.sent_size {
            self.sent_size = wanted;
            self.page.view().resize(wanted.0, wanted.1);
        }
        self.sync_focus(window);
        if let Some(frame) = self.page.take_frame()
            && let Some(image) = render_image(frame)
            && let Some((old, _)) = self.picture.replace(image)
            && window.drop_image(old).is_err()
        {
            // The picture was already gone from the atlas.
        }
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, cx.entity()),
            cx,
        );
        if let Some((image, scale)) = &self.picture {
            let extent = image.size(0);
            let shown = size(
                px(extent.width.0 as f32 / *scale as f32),
                px(extent.height.0 as f32 / *scale as f32),
            );
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                if window
                    .paint_image(
                        Bounds::new(bounds.origin, shown),
                        gpui::Corners::default(),
                        Arc::clone(image),
                        0,
                        false,
                    )
                    .is_err()
                {
                    // The picture could not be uploaded; the next one is tried at the next frame.
                }
            });
        }
    }
}

impl WebSurface {
    /// Puts the page, a window of its own, over `bounds`, cut to what the window shows of it, and
    /// gives it the keyboard when the pane has it.
    fn place(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handle) = window_handle(window) else {
            return;
        };
        let rect = |bounds: Bounds<Pixels>| {
            let sides = [
                bounds.origin.x,
                bounds.origin.y,
                bounds.size.width,
                bounds.size.height,
            ]
            .map(|side| f64::from(f32::from(side)));
            Rect {
                x: sides[0],
                y: sides[1],
                width: sides[2],
                height: sides[3],
            }
        };
        // A page is over the window and outside GPUI's clipping, so it is cut here to what the
        // window shows of the pane, and it waits while a drag or a tooltip is over the panes.
        self.page.place(Placement {
            window: handle,
            page: rect(bounds),
            clip: rect(window.content_mask().bounds),
            scale: f64::from(window.scale_factor()),
            visible: !self.covered
                && !cx.has_active_drag()
                && !crate::views::overlay::covers(bounds, cx),
        });
        self.sync_focus(window);
    }

    /// Tells the page when the pane has or has lost the keyboard.
    fn sync_focus(&mut self, window: &Window) {
        let focused = self.focus.is_focused(window);
        if focused != self.told_focus {
            self.told_focus = focused;
            self.page.view().focus(focused);
        }
    }
}

/// The system's handle of the window, where a page is placed over it.
#[cfg(windows)]
fn window_handle(window: &Window) -> Option<WindowHandle> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // The window's own method of this name gives GPUI's handle; the trait's gives the system's.
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return None;
    };
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(WindowHandle(handle.hwnd.get())),
        _ => None,
    }
}

#[cfg(not(windows))]
fn window_handle(_: &Window) -> Option<WindowHandle> {
    None
}

impl Drop for WebSurface {
    /// A page that is a window of its own goes off the screen with the pane's surface.
    fn drop(&mut self) {
        self.page.hide();
    }
}

/// The pointer button the page knows, for a button of the window.
fn button_of(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Primary),
        MouseButton::Middle => Some(Button::Middle),
        MouseButton::Right => Some(Button::Secondary),
        MouseButton::Navigate(_) => None,
    }
}

/// The window's form of a picture of the page, and its device pixels for each logical pixel.
fn render_image(frame: Frame) -> Option<(Arc<RenderImage>, u32)> {
    let row = frame.width as usize * 4;
    let bytes = if frame.stride == row {
        frame.bytes
    } else {
        frame
            .bytes
            .chunks_exact(frame.stride)
            .flat_map(|line| &line[..row])
            .copied()
            .collect()
    };
    let image = image::RgbaImage::from_raw(frame.width, frame.height, bytes)?;
    Some((
        Arc::new(RenderImage::new(vec![image::Frame::new(image)])),
        frame.scale,
    ))
}

impl Render for WebSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        div()
            .id("web-surface")
            .size_full()
            .relative()
            .bg(rgb(self.palette.face))
            .track_focus(&self.focus)
            .key_context("WebSurface")
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(Self::mouse_down),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(Self::mouse_down),
            )
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up_out))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, cx| {
                        entity.update(cx, |surface, cx| surface.paint(bounds, window, cx));
                    },
                )
                .absolute()
                .inset_0(),
            )
    }
}

/// The page takes the text a person composes: dead keys, input methods and emoji arrive here, as
/// editing of the page's own.
impl EntityInputHandler for WebSurface {
    fn text_for_range(
        &mut self,
        _span: Range<usize>,
        _snapped: &mut Option<Range<usize>>,
        _on: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<String> {
        // The page's own text is not read back to the input method.
        None
    }

    fn selected_text_range(
        &mut self,
        _direction: bool,
        _on: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _on: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _on: &mut Window, _context: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        _replaced: Option<Range<usize>>,
        text: &str,
        _on: &mut Window,
        _context: &mut Context<Self>,
    ) {
        self.page.view().insert_text(text);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _replaced: Option<Range<usize>>,
        _composing: &str,
        _selected: Option<Range<usize>>,
        _on: &mut Window,
        _context: &mut Context<Self>,
    ) {
        // Text being composed is shown by the input method until it is committed.
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        element: Bounds<Pixels>,
        _on: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(Bounds::new(element.origin, size(px(1.0), px(20.0))))
    }

    fn character_index_for_point(
        &mut self,
        _at: Point<Pixels>,
        _on: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// A new surface for the page of a pane.
pub(crate) fn new_surface(
    page: Arc<WebPage>,
    palette: &'static Palette,
    cx: &mut App,
) -> Entity<WebSurface> {
    cx.new(|cx| WebSurface::new(page, palette, cx))
}
