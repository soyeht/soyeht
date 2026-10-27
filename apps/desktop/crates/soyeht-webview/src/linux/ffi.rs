//! The C interface of GTK 3 and WebKitGTK 4.1, loaded when the first view opens. Only what the
//! web view uses is declared. Each signature is the one of the library's public headers; the
//! libraries keep that ABI for as long as their major version lasts.

use crate::{Error, Result};
use libloading::Library;
use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};

pub type Ptr = *mut c_void;
pub type GBool = c_int;
pub type GType = usize;
pub type Callback = unsafe extern "C" fn(Ptr);
pub type SourceFn = unsafe extern "C" fn(Ptr) -> GBool;
pub type AsyncReady = unsafe extern "C" fn(Ptr, Ptr, Ptr);
/// `GClosureNotify`: the data of a signal handler and its closure.
pub type ClosureNotify = unsafe extern "C" fn(Ptr, Ptr);

pub const FALSE: GBool = 0;
pub const TRUE: GBool = 1;
pub const PRIORITY_DEFAULT: c_int = 0;

/// The libraries, by the names a distribution installs them under. The first that loads is used.
const GTK: &[&str] = &["libgtk-3.so.0"];
const GDK: &[&str] = &["libgdk-3.so.0"];
const GLIB: &[&str] = &["libglib-2.0.so.0"];
const GOBJECT: &[&str] = &["libgobject-2.0.so.0"];
const CAIRO: &[&str] = &["libcairo.so.2"];
const WEBKIT: &[&str] = &["libwebkit2gtk-4.1.so.0", "libwebkit2gtk-4.0.so.37"];
const JSC: &[&str] = &[
    "libjavascriptcoregtk-4.1.so.0",
    "libjavascriptcoregtk-4.0.so.18",
];

struct Libraries {
    gtk: Library,
    gdk: Library,
    glib: Library,
    gobject: Library,
    cairo: Library,
    webkit: Library,
    jsc: Library,
}

fn open(names: &[&str]) -> Result<Library> {
    for name in names {
        // SAFETY: loading a shared library runs its initializers, which for these system
        // libraries only register types and read no argument from this program.
        if let Ok(library) = unsafe { Library::new(name) } {
            return Ok(library);
        }
    }
    Err(Error::Unavailable(
        "the system web view (WebKitGTK) is not installed",
    ))
}

macro_rules! symbols {
    ($($library:ident { $($field:ident ( $($argument:ty),* ) $(-> $returns:ty)? ;)* })*) => {
        /// Every function the web view calls.
        pub struct Api {
            $($(pub $field: unsafe extern "C" fn($($argument),*) $(-> $returns)?,)*)*
            /// `g_object_new`, which takes a list of property names and values.
            pub object_new: unsafe extern "C" fn(GType, *const c_char, ...) -> Ptr,
            _libraries: Libraries,
        }

        impl Api {
            pub fn load() -> Result<Self> {
                let libraries = Libraries {
                    gtk: open(GTK)?,
                    gdk: open(GDK)?,
                    glib: open(GLIB)?,
                    gobject: open(GOBJECT)?,
                    cairo: open(CAIRO)?,
                    webkit: open(WEBKIT)?,
                    jsc: open(JSC)?,
                };
                $($(
                    let $field = {
                        // SAFETY: the symbol is declared with the signature of its public header.
                        let symbol = unsafe {
                            libraries.$library.get::<unsafe extern "C" fn($($argument),*) $(-> $returns)?>(
                                concat!(stringify!($field), "\0").as_bytes(),
                            )
                        }
                        .map_err(|_| Error::Unavailable("the system web view has an unexpected version"))?;
                        *symbol
                    };
                )*)*
                let object_new = {
                    // SAFETY: `g_object_new` is variadic in its header and is declared so here.
                    let symbol = unsafe {
                        libraries.gobject.get::<unsafe extern "C" fn(GType, *const c_char, ...) -> Ptr>(
                            b"g_object_new\0",
                        )
                    }
                    .map_err(|_| Error::Unavailable("the system web view has an unexpected version"))?;
                    *symbol
                };
                Ok(Self { $($($field,)*)* object_new, _libraries: libraries })
            }
        }
    };
}

symbols! {
    glib {
        g_main_loop_new(Ptr, GBool) -> Ptr;
        g_main_loop_run(Ptr);
        g_idle_add_full(c_int, SourceFn, Ptr, Option<Callback>) -> c_uint;
        g_timeout_add_full(c_int, c_uint, SourceFn, Ptr, Option<Callback>) -> c_uint;
        g_source_remove(c_uint) -> GBool;
        g_free(Ptr);
        g_error_free(Ptr);
        g_list_length(Ptr) -> c_uint;
        g_list_free_full(Ptr, Callback);
        g_get_monotonic_time() -> i64;
    }
    gobject {
        g_signal_connect_data(Ptr, *const c_char, Ptr, Ptr, Option<ClosureNotify>, c_int) -> c_ulong;
        g_signal_handlers_disconnect_matched(Ptr, c_uint, c_uint, c_uint, Ptr, Ptr, Ptr) -> c_uint;
        g_object_unref(Ptr);
        g_object_ref(Ptr) -> Ptr;
    }
    gtk {
        gtk_init_check(*mut c_int, Ptr) -> GBool;
        gtk_offscreen_window_new() -> Ptr;
        gtk_offscreen_window_get_surface(Ptr) -> Ptr;
        gtk_container_add(Ptr, Ptr);
        gtk_widget_show_all(Ptr);
        gtk_widget_set_size_request(Ptr, c_int, c_int);
        gtk_widget_destroy(Ptr);
        gtk_widget_grab_focus(Ptr);
        gtk_widget_get_window(Ptr) -> Ptr;
        gtk_window_set_default_size(Ptr, c_int, c_int);
        gtk_window_resize(Ptr, c_int, c_int);
        gtk_main_do_event(Ptr);
    }
    gdk {
        gdk_event_new(c_int) -> Ptr;
        gdk_event_free(Ptr);
        gdk_event_set_device(Ptr, Ptr);
        gdk_display_get_default() -> Ptr;
        gdk_display_get_default_seat(Ptr) -> Ptr;
        gdk_seat_get_pointer(Ptr) -> Ptr;
        gdk_seat_get_keyboard(Ptr) -> Ptr;
        gdk_keymap_get_for_display(Ptr) -> Ptr;
        gdk_keymap_get_entries_for_keyval(Ptr, c_uint, *mut Ptr, *mut c_int) -> GBool;
    }
    cairo {
        cairo_surface_flush(Ptr);
        cairo_surface_destroy(Ptr);
        cairo_surface_get_device_scale(Ptr, *mut f64, *mut f64);
        cairo_image_surface_get_data(Ptr) -> *const u8;
        cairo_image_surface_get_width(Ptr) -> c_int;
        cairo_image_surface_get_height(Ptr) -> c_int;
        cairo_image_surface_get_stride(Ptr) -> c_int;
    }
    webkit {
        webkit_website_data_manager_get_type() -> GType;
        webkit_website_data_manager_get_cookie_manager(Ptr) -> Ptr;
        webkit_cookie_manager_set_persistent_storage(Ptr, *const c_char, c_int);
        webkit_cookie_manager_get_cookies(Ptr, *const c_char, Ptr, AsyncReady, Ptr);
        webkit_cookie_manager_get_cookies_finish(Ptr, Ptr, *mut Ptr) -> Ptr;
        webkit_web_context_new_with_website_data_manager(Ptr) -> Ptr;
        webkit_web_context_new_ephemeral() -> Ptr;
        webkit_web_context_get_website_data_manager(Ptr) -> Ptr;
        webkit_web_view_new_with_context(Ptr) -> Ptr;
        webkit_web_view_load_uri(Ptr, *const c_char);
        webkit_web_view_reload(Ptr);
        webkit_web_view_reload_bypass_cache(Ptr);
        webkit_web_view_go_back(Ptr);
        webkit_web_view_go_forward(Ptr);
        webkit_web_view_can_go_back(Ptr) -> GBool;
        webkit_web_view_can_go_forward(Ptr) -> GBool;
        webkit_web_view_is_loading(Ptr) -> GBool;
        webkit_web_view_get_estimated_load_progress(Ptr) -> f64;
        webkit_web_view_get_uri(Ptr) -> *const c_char;
        webkit_web_view_get_title(Ptr) -> *const c_char;
        webkit_web_view_evaluate_javascript(Ptr, *const c_char, isize, *const c_char, *const c_char, Ptr, AsyncReady, Ptr);
        webkit_web_view_evaluate_javascript_finish(Ptr, Ptr, *mut Ptr) -> Ptr;
        webkit_web_view_get_snapshot(Ptr, c_int, c_int, Ptr, AsyncReady, Ptr);
        webkit_web_view_get_snapshot_finish(Ptr, Ptr, *mut Ptr) -> Ptr;
        webkit_web_view_execute_editing_command(Ptr, *const c_char);
        webkit_web_view_execute_editing_command_with_argument(Ptr, *const c_char, *const c_char);
        webkit_uri_request_get_uri(Ptr) -> *const c_char;
        webkit_navigation_policy_decision_get_navigation_action(Ptr) -> Ptr;
        webkit_navigation_action_get_request(Ptr) -> Ptr;
        webkit_response_policy_decision_is_mime_type_supported(Ptr) -> GBool;
        webkit_policy_decision_ignore(Ptr);
        webkit_permission_request_deny(Ptr);
        webkit_script_dialog_get_dialog_type(Ptr) -> c_int;
        webkit_script_dialog_confirm_set_confirmed(Ptr, GBool);
        webkit_option_menu_close(Ptr);
        webkit_file_chooser_request_cancel(Ptr);
        soup_cookie_free(Ptr);
    }
    jsc {
        jsc_value_to_string(Ptr) -> *mut c_char;
    }
}
