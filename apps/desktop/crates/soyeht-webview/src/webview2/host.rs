//! The window that holds one page: a top-level window with no frame, owned by the program's
//! window so it stays above it, placed over the pane. The page is the system web view's own child
//! of it (ADR 0124). A child of the program's window itself would not show: its window is
//! composed with DirectComposition above its children.

use super::thread::Ui;
use super::wide::{pcwstr, wide};
use crate::{
    Error, Placement, Result, WindowHandle,
    placement::{Layout, layout},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Gdi::ClientToScreen,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, EVENT_OBJECT_LOCATIONCHANGE,
                GW_OWNER, GWLP_HWNDPARENT, GetWindow, GetWindowThreadProcessId, HWND_TOP, IsIconic,
                IsWindow, OBJID_WINDOW, PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE,
                SWP_NOACTIVATE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_CLOSE, WM_ERASEBKGND,
                WNDCLASSEXW, WNDPROC, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

const CLASS: &str = "SoyehtNextWebHost";
/// Where a page that is drawn for a picture and not for the person sits: left of every screen.
const OFF_SCREEN: i32 = -20_000;

/// Makes the window class of the hosts, once per thread that makes hosts.
pub(super) fn register_hosts() -> Result<()> {
    register_class(pcwstr(&wide(CLASS)), Some(window_proc))
}

/// Registers a window class of this program with `procedure` for its messages.
pub(super) fn register_class(name: PCWSTR, procedure: WNDPROC) -> Result<()> {
    // SAFETY: the module handle of this program is always available.
    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|_| Error::Unavailable("the program's module is not available"))?;
    let class = WNDCLASSEXW {
        cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or_default(),
        lpfnWndProc: procedure,
        hInstance: instance.into(),
        lpszClassName: name,
        ..Default::default()
    };
    // SAFETY: `class` is complete, and its name outlives the call, which copies it.
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(Error::Unavailable("a web view window class was refused"));
    }
    Ok(())
}

/// A window of a class this program registered, `width` by `height` (the size is in `size`).
pub(super) fn create_window(
    class: PCWSTR,
    style: WINDOW_STYLE,
    extended: WINDOW_EX_STYLE,
    size: (i32, i32),
    parent: Option<HWND>,
) -> Result<HWND> {
    // SAFETY: the module handle of this program is always available.
    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|_| Error::Unavailable("the program's module is not available"))?;
    // SAFETY: a plain window of a registered class, whose name the caller keeps alive.
    unsafe {
        CreateWindowExW(
            extended,
            class,
            w!(""),
            style,
            0,
            0,
            size.0,
            size.1,
            parent,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(|_| Error::Unavailable("a web view window could not be made"))
}

/// A host window, hidden, at the size the page lays out for until it is placed.
pub(super) fn create(width: i32, height: i32) -> Result<HWND> {
    // A tool window has no place on the task bar or in the switcher.
    create_window(
        pcwstr(&wide(CLASS)),
        WS_POPUP | WS_CLIPCHILDREN,
        WS_EX_TOOLWINDOW,
        (width, height),
        None,
    )
}

pub(super) fn destroy(host: HWND) {
    // SAFETY: the window is this thread's; a window already gone is not destroyed again.
    if unsafe { IsWindow(Some(host)) }.as_bool() && unsafe { DestroyWindow(host) }.is_err() {
        // The window went while it was being destroyed: it is gone either way.
    }
}

/// The window's own owner, after a close the person asked of the host: the program's window.
unsafe extern "system" fn window_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        // The page paints itself; nothing of the host is left to draw.
        WM_ERASEBKGND => LRESULT(1),
        // Alt+F4 or a close from the system menu asks the program's window to close, not this one.
        WM_CLOSE => {
            // SAFETY: the owner is looked up and written to by handle, which the system checks.
            let owner = unsafe { GetWindow(window, GW_OWNER) }.unwrap_or_default();
            if !owner.0.is_null() {
                // SAFETY: as above.
                if unsafe { PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_err() {
                    // The owner is gone, and with it the reason to close.
                }
            }
            LRESULT(0)
        }
        // SAFETY: the default procedure of the system, with this call's own arguments.
        _ => unsafe { DefWindowProcW(window, message, w, l) },
    }
}

/// Where the client area of `window` starts on the screen, in device pixels.
pub(super) fn client_origin(window: WindowHandle) -> (i32, i32) {
    let owner = HWND(window.0 as *mut std::ffi::c_void);
    if owner.0.is_null() {
        return (0, 0);
    }
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: a handle that is not a window makes the call fail and leave the origin at zero.
    if unsafe { ClientToScreen(owner, &raw mut origin) }.as_bool() {
        (origin.x, origin.y)
    } else {
        (0, 0)
    }
}

/// Whether the window is minimized, which has it report a place no screen has.
pub(super) fn is_minimized(window: WindowHandle) -> bool {
    // SAFETY: a query about a window, which the system checks.
    unsafe { IsIconic(HWND(window.0 as *mut std::ffi::c_void)) }.as_bool()
}

/// Has the thread hear of the window `owner` moving or changing its size, which moves the pages
/// that sit over it. One hook for each window.
pub(super) fn follow_owner(ui: &Ui, owner: WindowHandle) {
    if owner.0 == 0 || ui.hooks.borrow().contains_key(&owner.0) {
        return;
    }
    let window = HWND(owner.0 as *mut std::ffi::c_void);
    let mut process = 0;
    // SAFETY: a query about a window, which the system checks.
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&raw mut process)) };
    if thread == 0 {
        return;
    }
    // SAFETY: the callback is this module's, runs on this thread through its message loop, and
    // the hook is kept in the thread's table until the window is gone.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(moved),
            process,
            thread,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if !hook.0.is_null() {
        ui.hooks.borrow_mut().insert(owner.0, hook);
    }
}

/// Lets go of the hooks of windows that no page sits over any more (a window that was closed).
pub(super) fn release_owners(ui: &Ui) {
    let in_use: Vec<isize> = ui
        .views()
        .iter()
        .filter_map(|view| view.placement.get())
        .map(|placement| placement.window.0)
        .collect();
    ui.hooks.borrow_mut().retain(|owner, hook| {
        if in_use.contains(owner) {
            return true;
        }
        // SAFETY: the hook was made by `follow_owner` on this thread and is let go once.
        if !unsafe { UnhookWinEvent(*hook) }.as_bool() {
            // A hook that is already gone is gone.
        }
        false
    });
}

/// A window of the program moved or changed its size: the pages over it follow.
unsafe extern "system" fn moved(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if object != OBJID_WINDOW.0 {
        return;
    }
    let Some(ui) = super::thread::current() else {
        return;
    };
    for view in ui.views() {
        if view
            .placement
            .get()
            .is_some_and(|placement| placement.window.0 == window.0 as isize)
        {
            view.follow();
        }
    }
}

/// Shows the host where no screen is, at the size the page lays out for.
pub(super) fn reveal_off_screen(host: HWND, width: i32, height: i32) {
    // SAFETY: the host is this thread's window; a window that is gone is not moved.
    unsafe {
        if SetWindowPos(
            host,
            None,
            OFF_SCREEN,
            OFF_SCREEN,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .is_err()
        {
            // A host that is gone is not moved.
        }
        if ShowWindow(host, SW_SHOWNOACTIVATE).as_bool() {
            // The call says whether the window was shown before, which is no news.
        }
    }
}

pub(super) fn hide(host: HWND) {
    // SAFETY: the host is this thread's window.
    if unsafe { ShowWindow(host, SW_HIDE) }.as_bool() {
        // The call says whether the window was shown before, which is no news.
    }
}

/// Puts the host over the part of its pane that is on screen, and shows or hides it. Returns
/// where the page goes inside the host.
pub(super) fn place(host: HWND, placement: &Placement) -> Layout {
    let owner = HWND(placement.window.0 as *mut std::ffi::c_void);
    let found = layout(placement, client_origin(placement.window));
    // SAFETY: the host is this thread's window; an owner that is not a window is refused by the
    // system and leaves the host unowned.
    unsafe {
        // The owner is set when it changes only: setting it restacks the window.
        if GetWindow(host, GW_OWNER).unwrap_or_default() != owner {
            SetWindowLongPtrW(host, GWLP_HWNDPARENT, owner.0 as isize);
        }
        if SetWindowPos(
            host,
            Some(HWND_TOP),
            found.host.x,
            found.host.y,
            found.host.width,
            found.host.height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .is_err()
        {
            // A host that is gone is not placed; the next call of the view says it is closed.
        }
        let command = if found.shown {
            SW_SHOWNOACTIVATE
        } else {
            SW_HIDE
        };
        if ShowWindow(host, command).as_bool() {
            // The call says whether the window was shown before, which is no news.
        }
    }
    found
}
