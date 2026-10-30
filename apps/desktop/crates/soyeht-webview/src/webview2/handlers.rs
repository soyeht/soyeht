//! What the page asks of the program, and the answers: where it may go, what it may not do, and
//! what the window is told of it. A page asks nothing of the person here (ADR 0124): a request for
//! a permission is refused, a dialog is closed, a download is not saved, a new window opens in the
//! same page.

use super::view::{ViewCtx, target};
use crate::{Error, Navigation, Result, policy};
use std::rc::{Rc, Weak};
use webview2_com::Microsoft::Web::WebView2::Win32;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, DocumentTitleChangedEventHandler,
    DownloadStartingEventHandler, FocusChangedEventHandler, HistoryChangedEventHandler,
    LaunchingExternalUriSchemeEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_STATE_DENY, COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT,
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD, ICoreWebView2_4, ICoreWebView2_18,
        ICoreWebView2Settings, ICoreWebView2Settings3, ICoreWebView2Settings4,
        ICoreWebView2Settings5, ICoreWebView2Settings6,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    ScriptDialogOpeningEventHandler, SourceChangedEventHandler, take_pwstr,
};
use windows::core::{Interface, PWSTR};

/// The address a call of the runtime gave, and frees it; empty when the call failed.
fn address_of(result: windows::core::Result<()>, uri: PWSTR) -> String {
    if result.is_ok() && !uri.is_null() {
        take_pwstr(uri)
    } else {
        String::new()
    }
}

/// Connects the page's events and applies its settings.
pub(super) fn connect(view: &Rc<ViewCtx>) -> Result<()> {
    settings(view)?;
    let mut token = 0_i64;
    let page = &view.webview;
    // SAFETY: each handler owns a weak reference to the context, which the thread's table keeps
    // alive until the page is closed; the token is the call's own.
    let added = unsafe {
        let weak = |view: &Rc<ViewCtx>| Rc::downgrade(view);
        page.add_NavigationStarting(&navigation_starting(weak(view)), &raw mut token)
            .and_then(|()| {
                page.add_NavigationCompleted(&navigation_completed(weak(view)), &raw mut token)
            })
            .and_then(|()| page.add_SourceChanged(&source_changed(weak(view)), &raw mut token))
            .and_then(|()| {
                page.add_DocumentTitleChanged(&title_changed(weak(view)), &raw mut token)
            })
            .and_then(|()| page.add_HistoryChanged(&history_changed(weak(view)), &raw mut token))
            .and_then(|()| page.add_NewWindowRequested(&new_window(weak(view)), &raw mut token))
            .and_then(|()| page.add_PermissionRequested(&permission(), &raw mut token))
            .and_then(|()| page.add_ScriptDialogOpening(&dialog(), &raw mut token))
            .and_then(|()| page.add_ProcessFailed(&failed(weak(view)), &raw mut token))
    };
    added.map_err(|_| Error::Unavailable("the web view events could not be connected"))?;
    // The person's own keys and clicks in the page are told to the window: it hands the pane to
    // them, as it does when the pointer is over a page it draws itself.
    // SAFETY: as above.
    let heard = unsafe {
        view.controller
            .add_GotFocus(&got_focus(Rc::downgrade(view)), &raw mut token)
            .and_then(|()| {
                view.controller
                    .add_AcceleratorKeyPressed(&key_pressed(Rc::downgrade(view)), &raw mut token)
            })
    };
    heard.map_err(|_| Error::Unavailable("the web view events could not be connected"))?;
    keep_to_the_web(view, &mut token)?;
    Ok(())
}

/// What a page may not take out of the pane, so that no way of leaving it is left open: a frame
/// goes only where the pane goes, a download is not saved and a program of the system is not
/// started. A runtime that lacks one of these events is not used: a page there could do what
/// is refused here.
fn keep_to_the_web(view: &ViewCtx, token: &mut i64) -> Result<()> {
    let refused = || Error::Unavailable("the web view lacks what keeps a page in the pane");
    // SAFETY: the handlers own nothing of the context; the token is the call's own.
    unsafe {
        view.webview
            .add_FrameNavigationStarting(&frame_navigation_starting(), token)
            .map_err(|_| refused())?;
        view.webview
            .cast::<ICoreWebView2_4>()
            .and_then(|page| page.add_DownloadStarting(&download(), token))
            .map_err(|_| refused())?;
        view.webview
            .cast::<ICoreWebView2_18>()
            .and_then(|page| page.add_LaunchingExternalUriScheme(&external_scheme(), token))
            .map_err(|_| refused())?;
    }
    Ok(())
}

/// A frame of the page goes where a frame may (`policy::allows_frame`: the pane's addresses and
/// the `data:` and `about:srcdoc` a page embeds), and nowhere else.
fn frame_navigation_starting() -> Win32::ICoreWebView2NavigationStartingEventHandler {
    NavigationStartingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut uri = PWSTR::null();
            // SAFETY: the arguments are alive for the call of the handler.
            let address = address_of(unsafe { args.Uri(&raw mut uri) }, uri);
            // SAFETY: as above.
            if !policy::allows_frame(&address) && unsafe { args.SetCancel(true) }.is_err() {
                // The frame went on its way before it could be stopped.
            }
        }
        Ok(())
    }))
}

/// A link to a program of the system (a mail client, a store) is not followed.
fn external_scheme() -> Win32::ICoreWebView2LaunchingExternalUriSchemeEventHandler {
    LaunchingExternalUriSchemeEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            // SAFETY: the arguments are alive for the call of the handler.
            if unsafe { args.SetCancel(true) }.is_err() {
                // The program was started before it could be stopped.
            }
        }
        Ok(())
    }))
}

/// What the page may do: no tools for a person to open it, no objects of ours in it, no windows
/// of its own.
fn settings(view: &ViewCtx) -> Result<()> {
    // SAFETY: the web view is alive; each setter writes a flag of its own settings object.
    let settings = unsafe { view.webview.Settings() }
        .map_err(|_| Error::Unavailable("the web view settings could not be read"))?;
    // SAFETY: as above.
    let applied = unsafe {
        settings
            .SetAreDevToolsEnabled(false)
            .and_then(|()| settings.SetAreDefaultContextMenusEnabled(false))
            .and_then(|()| settings.SetAreHostObjectsAllowed(false))
            .and_then(|()| settings.SetIsWebMessageEnabled(false))
            .and_then(|()| settings.SetIsStatusBarEnabled(false))
            .and_then(|()| settings.SetAreDefaultScriptDialogsEnabled(false))
            .and_then(|()| settings.SetIsZoomControlEnabled(false))
    };
    applied.map_err(|_| Error::Unavailable("the web view settings could not be set"))?;
    optional(&settings, |s: &ICoreWebView2Settings3| {
        // SAFETY: the interface is alive.
        unsafe { s.SetAreBrowserAcceleratorKeysEnabled(false) }
    });
    optional(&settings, |s: &ICoreWebView2Settings4| {
        // SAFETY: the interface is alive.
        unsafe {
            s.SetIsPasswordAutosaveEnabled(false)
                .and_then(|()| s.SetIsGeneralAutofillEnabled(false))
        }
    });
    optional(&settings, |s: &ICoreWebView2Settings5| {
        // SAFETY: the interface is alive.
        unsafe { s.SetIsPinchZoomEnabled(false) }
    });
    optional(&settings, |s: &ICoreWebView2Settings6| {
        // SAFETY: the interface is alive.
        unsafe { s.SetIsSwipeNavigationEnabled(false) }
    });
    Ok(())
}

/// Applies a setting of a newer version of the settings interface. A runtime that lacks it keeps
/// its default, which is the same page with one more thing a person can do.
fn optional<T: Interface>(
    settings: &ICoreWebView2Settings,
    apply: impl FnOnce(&T) -> windows::core::Result<()>,
) {
    if let Ok(newer) = settings.cast::<T>()
        && apply(&newer).is_err()
    {
        // The runtime refused the setting; the default stays.
    }
}

fn navigation_starting(weak: Weak<ViewCtx>) -> Win32::ICoreWebView2NavigationStartingEventHandler {
    NavigationStartingEventHandler::create(Box::new(move |_, args| {
        let (Some(view), Some(args)) = (weak.upgrade(), args) else {
            return Ok(());
        };
        let mut uri = PWSTR::null();
        // SAFETY: the arguments are alive for the call of the handler.
        let address = address_of(unsafe { args.Uri(&raw mut uri) }, uri);
        if !policy::allows(&address) {
            // SAFETY: as above.
            if unsafe { args.SetCancel(true) }.is_err() {
                // The navigation went on its way before it could be stopped.
            }
            return Ok(());
        }
        let mut redirected = windows::core::BOOL(0);
        // SAFETY: as above.
        if unsafe { args.IsRedirected(&raw mut redirected) }.is_ok() && redirected.as_bool() {
            return Ok(());
        }
        view.loading.set(true);
        view.shared.navigation_began();
        view.refresh_state();
        Ok(())
    }))
}

/// A function that makes the handler of an event that only has `view` do something about the page
/// in `$body`; the event's own arguments are of no use to it.
macro_rules! on_view {
    ($name:ident, $handler:ident, $interface:ident, |$view:ident| $body:block) => {
        fn $name(weak: Weak<ViewCtx>) -> Win32::$interface {
            $handler::create(Box::new(move |_, _| {
                if let Some($view) = weak.upgrade() $body
                Ok(())
            }))
        }
    };
}

on_view!(
    navigation_completed,
    NavigationCompletedEventHandler,
    ICoreWebView2NavigationCompletedEventHandler,
    |view| {
        view.loading.set(false);
        view.refresh_state();
    }
);
on_view!(
    source_changed,
    SourceChangedEventHandler,
    ICoreWebView2SourceChangedEventHandler,
    |view| { view.refresh_state() }
);
on_view!(
    title_changed,
    DocumentTitleChangedEventHandler,
    ICoreWebView2DocumentTitleChangedEventHandler,
    |view| { view.refresh_state() }
);
on_view!(
    history_changed,
    HistoryChangedEventHandler,
    ICoreWebView2HistoryChangedEventHandler,
    |view| { view.refresh_state() }
);
on_view!(
    failed,
    ProcessFailedEventHandler,
    ICoreWebView2ProcessFailedEventHandler,
    |view| {
        view.loading.set(false);
        view.refresh_state();
    }
);

/// A page that asks for a new window gets its address opened here instead: a web pane has no
/// second window to put it in.
fn new_window(weak: Weak<ViewCtx>) -> Win32::ICoreWebView2NewWindowRequestedEventHandler {
    NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
        let (Some(view), Some(args)) = (weak.upgrade(), args) else {
            return Ok(());
        };
        // SAFETY: the arguments are alive for the call of the handler.
        unsafe {
            if args.SetHandled(true).is_err() {
                // The runtime opens its own window then; the person closes it.
            }
            let mut uri = PWSTR::null();
            let address = address_of(args.Uri(&raw mut uri), uri);
            if policy::allows_request(&address)
                && let Ok(target) = target(Navigation::Url(&address))
            {
                view.navigate(&target);
            }
        }
        Ok(())
    }))
}

/// A page's request for the camera, the position, notifications or the like is refused: the pane
/// asks the person nothing yet.
fn permission() -> Win32::ICoreWebView2PermissionRequestedEventHandler {
    PermissionRequestedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            // SAFETY: the arguments are alive for the call of the handler.
            if unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY) }.is_err() {
                // The request was answered already.
            }
        }
        Ok(())
    }))
}

/// An alert is read and closed, a question is answered "no", and leaving a page is allowed:
/// a dialog of the system would open a window the pane does not own.
fn dialog() -> Win32::ICoreWebView2ScriptDialogOpeningEventHandler {
    ScriptDialogOpeningEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        // SAFETY: the arguments are alive for the call of the handler.
        unsafe {
            let mut kind = Default::default();
            if args.Kind(&raw mut kind).is_ok()
                && (kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT
                    || kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD)
                && args.Accept().is_err()
            {
                // The dialog was closed already.
            }
        }
        Ok(())
    }))
}

/// A web pane reads pages, it does not save files: a download is stopped.
fn download() -> Win32::ICoreWebView2DownloadStartingEventHandler {
    DownloadStartingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            // SAFETY: the arguments are alive for the call of the handler.
            unsafe {
                if args.SetCancel(true).is_err() || args.SetHandled(true).is_err() {
                    // The download had started when it was refused; its file stays empty.
                }
            }
        }
        Ok(())
    }))
}

// The page took the keyboard. When the program asked for it just now it is not the person's.
on_view!(
    got_focus,
    FocusChangedEventHandler,
    ICoreWebView2FocusChangedEventHandler,
    |view| {
        if !view.focus_was_asked_for() {
            view.person();
        }
    }
);
// A key pressed in the page, before the page handles it.
on_view!(
    key_pressed,
    AcceleratorKeyPressedEventHandler,
    ICoreWebView2AcceleratorKeyPressedEventHandler,
    |view| { view.person() }
);
