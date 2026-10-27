//! The person's pointer and keys, as the events GTK delivers to a page. The engine receives them
//! as it does from a real device, so the page sees what it sees from a person at a screen: events
//! that are trusted, with a pointer that moves before it clicks.

use super::{
    Gtk,
    ffi::{self, Api, Ptr},
    view::ViewCtx,
};
use crate::{Button, KeyInput, Modifiers, Pointer, ScrollInput};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::{c_int, c_uint},
    ptr,
};

/// `GdkEventType`, the ones sent.
const MOTION_NOTIFY: c_int = 3;
const BUTTON_PRESS: c_int = 4;
const BUTTON_RELEASE: c_int = 7;
const KEY_PRESS: c_int = 8;
const KEY_RELEASE: c_int = 9;
const FOCUS_CHANGE: c_int = 12;
const SCROLL: c_int = 31;
/// `GDK_SCROLL_SMOOTH`.
const SCROLL_SMOOTH: c_int = 4;

/// `GdkModifierType`.
const SHIFT_MASK: u32 = 1 << 0;
const CONTROL_MASK: u32 = 1 << 2;
const ALT_MASK: u32 = 1 << 3;
const BUTTON1_MASK: u32 = 1 << 8;
const BUTTON2_MASK: u32 = 1 << 9;
const BUTTON3_MASK: u32 = 1 << 10;
const SUPER_MASK: u32 = 1 << 26;

/// The start of every `GdkEvent`. The structures below repeat these three fields rather than
/// nest this one: C puts the next field right after `send_event`, a nested structure would pad.
#[repr(C)]
struct Header {
    _kind: c_int,
    window: Ptr,
    send_event: i8,
}

/// `GdkEventButton`.
#[repr(C)]
struct ButtonEvent {
    kind: c_int,
    window: Ptr,
    send_event: i8,
    time: u32,
    x: f64,
    y: f64,
    axes: Ptr,
    state: c_uint,
    button: c_uint,
    device: Ptr,
    x_root: f64,
    y_root: f64,
}

/// `GdkEventMotion`.
#[repr(C)]
struct MotionEvent {
    kind: c_int,
    window: Ptr,
    send_event: i8,
    time: u32,
    x: f64,
    y: f64,
    axes: Ptr,
    state: c_uint,
    is_hint: i16,
    device: Ptr,
    x_root: f64,
    y_root: f64,
}

/// `GdkEventScroll`.
#[repr(C)]
struct ScrollEvent {
    kind: c_int,
    window: Ptr,
    send_event: i8,
    time: u32,
    x: f64,
    y: f64,
    state: c_uint,
    direction: c_int,
    device: Ptr,
    x_root: f64,
    y_root: f64,
    delta_x: f64,
    delta_y: f64,
}

/// `GdkEventKey`.
#[repr(C)]
struct KeyEvent {
    kind: c_int,
    window: Ptr,
    send_event: i8,
    time: u32,
    state: c_uint,
    keyval: c_uint,
    length: c_int,
    string: Ptr,
    hardware_keycode: u16,
    group: u8,
}

/// `GdkEventFocus`.
#[repr(C)]
struct FocusEvent {
    kind: c_int,
    window: Ptr,
    send_event: i8,
    focus_in: i16,
}

/// `GdkKeymapKey`.
#[repr(C)]
struct KeymapKey {
    keycode: c_uint,
    group: c_int,
    level: c_int,
}

impl Gtk {
    pub(super) fn new(api: &'static Api) -> Self {
        // SAFETY: GTK is initialized; the display, seat and devices belong to it and live as long
        // as the process.
        let (pointer, keyboard, keymap) = unsafe {
            let display = (api.gdk_display_get_default)();
            let seat = if display.is_null() {
                ptr::null_mut()
            } else {
                (api.gdk_display_get_default_seat)(display)
            };
            let (pointer, keyboard) = if seat.is_null() {
                (ptr::null_mut(), ptr::null_mut())
            } else {
                (
                    (api.gdk_seat_get_pointer)(seat),
                    (api.gdk_seat_get_keyboard)(seat),
                )
            };
            let keymap = if display.is_null() {
                ptr::null_mut()
            } else {
                (api.gdk_keymap_get_for_display)(display)
            };
            (pointer, keyboard, keymap)
        };
        Self {
            api,
            contexts: RefCell::new(HashMap::new()),
            views: RefCell::new(HashMap::new()),
            pointer: Cell::new(pointer),
            keyboard: Cell::new(keyboard),
            keymap: Cell::new(keymap),
        }
    }

    /// The key a symbol is on, so the page's `code` names the key as well as its `key`.
    fn keycode_for(&self, keysym: u32) -> u16 {
        let keymap = self.keymap.get();
        if keymap.is_null() {
            return 0;
        }
        let mut keys: Ptr = ptr::null_mut();
        let mut count: c_int = 0;
        // SAFETY: the keymap is GTK's; the array it returns is read, then freed with `g_free`.
        unsafe {
            let found = (self.api.gdk_keymap_get_entries_for_keyval)(
                keymap,
                keysym,
                &raw mut keys,
                &raw mut count,
            );
            if found == ffi::FALSE || keys.is_null() {
                return 0;
            }
            let code = if count > 0 {
                u16::try_from((*keys.cast::<KeymapKey>()).keycode).unwrap_or(0)
            } else {
                0
            };
            (self.api.g_free)(keys);
            code
        }
    }
}

fn modifier_bits(modifiers: Modifiers) -> u32 {
    let mut bits = 0;
    for (held, mask) in [
        (modifiers.shift, SHIFT_MASK),
        (modifiers.control, CONTROL_MASK),
        (modifiers.alt, ALT_MASK),
        (modifiers.logo, SUPER_MASK),
    ] {
        if held {
            bits |= mask;
        }
    }
    bits
}

fn button_number(button: Button) -> (c_uint, u32) {
    match button {
        Button::Primary => (1, BUTTON1_MASK),
        Button::Middle => (2, BUTTON2_MASK),
        Button::Secondary => (3, BUTTON3_MASK),
    }
}

/// Builds an event of `kind` for `widget`'s window, lets `fill` set its fields, and delivers it
/// as GTK delivers an event from the display.
fn emit<E>(gtk: &Gtk, widget: Ptr, kind: c_int, device: Ptr, fill: impl FnOnce(&mut E)) {
    let api = gtk.api;
    // SAFETY: the widget is alive. The event is made by GDK for `kind`, whose structure starts
    // with `Header` and is at least as large as `E`; it is freed once, after it is delivered.
    unsafe {
        let window = (api.gtk_widget_get_window)(widget);
        if window.is_null() {
            return;
        }
        let event = (api.gdk_event_new)(kind);
        if event.is_null() {
            return;
        }
        let header = &mut *event.cast::<Header>();
        header.window = (api.g_object_ref)(window);
        header.send_event = 1;
        fill(&mut *event.cast::<E>());
        if !device.is_null() {
            (api.gdk_event_set_device)(event, device);
        }
        (api.gtk_main_do_event)(event);
        (api.gdk_event_free)(event);
    }
}

/// The time of an event, in milliseconds on a clock that does not go back.
fn now(gtk: &Gtk) -> u32 {
    // SAFETY: a plain clock read.
    let micros = unsafe { (gtk.api.g_get_monotonic_time)() };
    // The wire time is 32 bits and wraps, as GDK's does.
    (micros / 1000) as u32
}

pub(super) fn pointer(gtk: &Gtk, view: &ViewCtx, pointer: Pointer, modifiers: Modifiers) {
    let time = now(gtk);
    let held = modifier_bits(modifiers);
    let device = gtk.pointer.get();
    match pointer {
        Pointer::Move { x, y } => {
            let state = held | view.buttons.get();
            emit::<MotionEvent>(gtk, view.webview(), MOTION_NOTIFY, device, |event| {
                (event.time, event.x, event.y, event.state) = (time, x, y, state);
                (event.x_root, event.y_root) = (x, y);
            });
        }
        Pointer::Down { x, y, button } | Pointer::Up { x, y, button } => {
            let pressed = matches!(pointer, Pointer::Down { .. });
            let (number, mask) = button_number(button);
            // A press is made with the buttons held before it; a release still holds its own.
            let state = held | view.buttons.get();
            let buttons = view.buttons.get();
            view.buttons.set(if pressed {
                buttons | mask
            } else {
                buttons & !mask
            });
            let kind = if pressed {
                BUTTON_PRESS
            } else {
                BUTTON_RELEASE
            };
            emit::<ButtonEvent>(gtk, view.webview(), kind, device, |event| {
                (event.time, event.x, event.y, event.state) = (time, x, y, state);
                (event.button, event.x_root, event.y_root) = (number, x, y);
            });
        }
    }
}

pub(super) fn scroll(gtk: &Gtk, view: &ViewCtx, scroll: ScrollInput, modifiers: Modifiers) {
    let time = now(gtk);
    let state = modifier_bits(modifiers) | view.buttons.get();
    emit::<ScrollEvent>(gtk, view.webview(), SCROLL, gtk.pointer.get(), |event| {
        (event.time, event.x, event.y, event.state) = (time, scroll.x, scroll.y, state);
        (event.x_root, event.y_root) = (scroll.x, scroll.y);
        (event.direction, event.delta_x, event.delta_y) = (SCROLL_SMOOTH, scroll.dx, scroll.dy);
    });
}

pub(super) fn key(gtk: &Gtk, view: &ViewCtx, key: KeyInput) {
    let time = now(gtk);
    let kind = if key.pressed { KEY_PRESS } else { KEY_RELEASE };
    let keycode = gtk.keycode_for(key.keysym.0);
    let state = modifier_bits(key.modifiers);
    emit::<KeyEvent>(gtk, view.webview(), kind, gtk.keyboard.get(), |event| {
        (event.time, event.state, event.keyval) = (time, state, key.keysym.0);
        event.hardware_keycode = keycode;
    });
}

/// The pane has the keyboard, or has not: the page's caret and `:focus` follow.
pub(super) fn focus(gtk: &Gtk, view: &ViewCtx, focused: bool) {
    if focused {
        // SAFETY: the web view is alive.
        unsafe { (gtk.api.gtk_widget_grab_focus)(view.webview()) };
    }
    emit::<FocusEvent>(
        gtk,
        view.window(),
        FOCUS_CHANGE,
        gtk.keyboard.get(),
        |event| {
            event.focus_in = i16::from(focused);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets `gdk/gdkevents.h` gives on a 64-bit system. A field a page of the library reads
    /// at another place is a crash in GDK, which the first click on a page showed.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_events_have_the_layout_of_gdks() {
        use std::mem::offset_of;
        assert_eq!(offset_of!(Header, window), 8);
        assert_eq!(offset_of!(Header, send_event), 16);
        assert_eq!(offset_of!(ButtonEvent, time), 20);
        assert_eq!(offset_of!(ButtonEvent, x), 24);
        assert_eq!(offset_of!(ButtonEvent, axes), 40);
        assert_eq!(offset_of!(ButtonEvent, state), 48);
        assert_eq!(offset_of!(ButtonEvent, button), 52);
        assert_eq!(offset_of!(ButtonEvent, device), 56);
        assert_eq!(offset_of!(ButtonEvent, x_root), 64);
        assert_eq!(offset_of!(MotionEvent, axes), 40);
        assert_eq!(offset_of!(MotionEvent, state), 48);
        assert_eq!(offset_of!(MotionEvent, device), 56);
        assert_eq!(offset_of!(MotionEvent, y_root), 72);
        assert_eq!(offset_of!(ScrollEvent, state), 40);
        assert_eq!(offset_of!(ScrollEvent, direction), 44);
        assert_eq!(offset_of!(ScrollEvent, device), 48);
        assert_eq!(offset_of!(ScrollEvent, delta_x), 72);
        assert_eq!(offset_of!(ScrollEvent, delta_y), 80);
        assert_eq!(offset_of!(KeyEvent, state), 24);
        assert_eq!(offset_of!(KeyEvent, keyval), 28);
        assert_eq!(offset_of!(KeyEvent, string), 40);
        assert_eq!(offset_of!(KeyEvent, hardware_keycode), 48);
        assert_eq!(offset_of!(FocusEvent, focus_in), 18);
    }

    #[test]
    fn modifiers_are_gdks_bits() {
        let all = Modifiers {
            shift: true,
            control: true,
            alt: true,
            logo: true,
        };
        assert_eq!(modifier_bits(all), 0x0400_000d);
        assert_eq!(modifier_bits(Modifiers::default()), 0);
    }

    #[test]
    fn buttons_are_numbered_as_gdk_numbers_them() {
        assert_eq!(button_number(Button::Primary), (1, 1 << 8));
        assert_eq!(button_number(Button::Middle), (2, 1 << 9));
        assert_eq!(button_number(Button::Secondary), (3, 1 << 10));
    }
}
