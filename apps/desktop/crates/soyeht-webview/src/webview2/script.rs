//! What the reader asks of a page that answers later: a script's value, in a world of the engine
//! the page cannot see, and the cookies of an address.
//!
//! The world is made by the engine's own protocol, in the process of the program: no port, no
//! pipe, no other program is involved, and no domain of the protocol is switched on, so the page
//! has nothing to observe. Four calls are made and no other: the frame tree, the world, a
//! script's evaluation in it, and a reload that bypasses the cache (ADR 0124).

use super::{
    reply_with,
    view::ViewCtx,
    wide::{pcwstr, wide},
};
use crate::{Error, Result, ScrollInput, world, world::Method};
use std::{cell::Cell, rc::Rc, sync::mpsc::Sender};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, GetCookiesCompletedHandler,
    Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2_2},
};
use windows::core::Interface;

/// One call of the engine's protocol; `done` hears of its answer on this thread.
fn call(
    webview: &ICoreWebView2,
    method: Method,
    arguments: &str,
    done: impl FnOnce(Result<String>) + 'static,
) {
    let (method, arguments) = (wide(method.name()), wide(arguments));
    type Done = Box<dyn FnOnce(Result<String>)>;
    let done: Rc<Cell<Option<Done>>> = Rc::new(Cell::new(Some(Box::new(done))));
    let heard = Rc::clone(&done);
    let handler =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, answer| {
            if let Some(done) = heard.take() {
                done(
                    result
                        .map(|()| answer)
                        .map_err(|_| Error::Page("the page did not answer")),
                );
            }
            Ok(())
        }));
    // SAFETY: the strings outlive the call, which copies them; the handler is the call's own.
    let started = unsafe {
        webview.CallDevToolsProtocolMethod(pcwstr(&method), pcwstr(&arguments), &handler)
    };
    if started.is_err() {
        // The call was never made, so its handler never runs: the caller is told here.
        if let Some(done) = done.take() {
            done(Err(Error::Page("the page did not answer")));
        }
    }
}

/// Runs `source` in the reader's world and answers with its value as text.
pub(super) fn run(view: &ViewCtx, source: &str, reply: Sender<Result<String>>) {
    let webview = view.webview.clone();
    let source = source.to_owned();
    call(&view.webview, Method::FrameTree, "{}", move |tree| {
        let frame = match tree.and_then(|answer| world::frame_id(&answer)) {
            Ok(frame) => frame,
            Err(error) => return reply_with(&reply, Err(error)),
        };
        let again = webview.clone();
        call(
            &webview,
            Method::CreateWorld,
            &world::create_world_arguments(&frame),
            move |made| {
                let context = match made.and_then(|answer| world::context_id(&answer)) {
                    Ok(context) => context,
                    Err(error) => return reply_with(&reply, Err(error)),
                };
                call(
                    &again,
                    Method::Evaluate,
                    &world::evaluate_arguments(context, &source),
                    move |answer| {
                        reply_with(&reply, answer.and_then(|text| world::value_text(&text)))
                    },
                );
            },
        );
    });
}

/// How far a line of a wheel moves a page, in pixels: what a browser scrolls a line.
const LINE_PIXELS: f64 = 40.0;

/// Scrolls what is at (`x`, `y`) of the page by `lines`, positive going down: the nearest
/// element that scrolls, or the page. Nothing is dispatched and nothing is left in the page.
pub(super) fn scroll(view: &ViewCtx, scroll: &ScrollInput) {
    let number = |value: f64| if value.is_finite() { value } else { 0.0 };
    let source = format!(
        "(function(a){{let e=document.elementFromPoint(a.x,a.y);\
         while(e&&e!==document.body&&e!==document.documentElement){{\
         if(/(auto|scroll|overlay)/.test(getComputedStyle(e).overflowY)&&e.scrollHeight>e.clientHeight)\
         {{e.scrollBy({{left:a.dx,top:a.dy,behavior:'instant'}});return 'ok';}}e=e.parentElement;}}\
         window.scrollBy({{left:a.dx,top:a.dy,behavior:'instant'}});return 'ok';}})\
         ({{x:{},y:{},dx:{},dy:{}}})",
        number(scroll.x),
        number(scroll.y),
        number(scroll.dx) * LINE_PIXELS,
        number(scroll.dy) * LINE_PIXELS,
    );
    // The answer is not wanted: whoever waits for the page to settle reads it.
    let (reply, _answer) = std::sync::mpsc::channel();
    run(view, &source, reply);
}

/// Reloads the page and does not use what the cache holds.
pub(super) fn reload_ignoring_cache(view: &ViewCtx) {
    call(
        &view.webview,
        Method::ReloadIgnoringCache,
        world::RELOAD_ARGUMENTS,
        |answer| {
            if answer.is_err() {
                // The page was gone before it reloaded.
            }
        },
    );
}

/// Whether the profile holds a cookie for `url`.
pub(super) fn cookies_present(view: &ViewCtx, url: &str, reply: Sender<Result<bool>>) {
    let manager = view
        .webview
        .cast::<ICoreWebView2_2>()
        // SAFETY: the web view is alive; the manager it gives is a reference of its own.
        .and_then(|webview| unsafe { webview.CookieManager() });
    let Ok(manager) = manager else {
        return reply_with(&reply, Err(Error::Page("the cookies could not be read")));
    };
    let url = wide(url);
    let answered = reply.clone();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |result, list| {
        let present = match (result, list) {
            (Ok(()), Some(list)) => {
                let mut count = 0_u32;
                // SAFETY: the list is alive for the call of the handler.
                unsafe { list.Count(&raw mut count) }
                    .map(|()| count > 0)
                    .map_err(|_| Error::Page("the cookies could not be read"))
            }
            _ => Err(Error::Page("the cookies could not be read")),
        };
        reply_with(&answered, present);
        Ok(())
    }));
    // SAFETY: the address outlives the call, which copies it; the handler is the call's own.
    if unsafe { manager.GetCookies(pcwstr(&url), &handler) }.is_err() {
        reply_with(&reply, Err(Error::Page("the cookies could not be read")));
    }
}
