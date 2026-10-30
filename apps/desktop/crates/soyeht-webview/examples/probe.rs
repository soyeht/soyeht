//! Opens one page in the system web view and reports what a reader gets from it. For trying the
//! web view on a machine: `probe PROFILE_DIR URL OUT_PREFIX`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use soyeht_webview::{
    Backend, Frame, Modifiers, Navigation, Placement, Rect, ScrollInput, ViewOptions, WebView,
    WindowHandle, embedded,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn save(frame: &Frame, path: &str) {
    let mut rgba = Vec::with_capacity(frame.bytes.len());
    for row in frame.bytes.chunks_exact(frame.stride) {
        for pixel in row[..frame.width as usize * 4].as_chunks::<4>().0 {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    image::RgbaImage::from_raw(frame.width, frame.height, rgba)
        .unwrap()
        .save(path)
        .unwrap();
    println!(
        "{path}: {}x{} scale {}",
        frame.width, frame.height, frame.scale
    );
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let (profile, url, prefix) = (&arguments[1], &arguments[2], &arguments[3]);
    let backend = Backend::get().unwrap();
    let view = backend
        .open(
            &ViewOptions {
                profile: Some(PathBuf::from(profile)),
                width: 1100,
                height: 760,
            },
            Arc::new(|_| {}),
        )
        .unwrap();
    // A page that is a window of its own is put over the screen, where the program would put it
    // over its pane.
    let pane = Rect {
        x: 40.0,
        y: 40.0,
        width: 1000.0,
        height: 640.0,
    };
    let placement = |visible: bool| Placement {
        window: WindowHandle(0),
        page: pane,
        clip: pane,
        scale: 1.0,
        visible,
    };
    if embedded() {
        view.place(placement(true));
    }
    view.navigate(Navigation::Url(url)).unwrap();
    for _ in 0..150 {
        std::thread::sleep(Duration::from_millis(200));
        let state = view.state();
        if !state.loading && !state.url.is_empty() {
            break;
        }
    }
    std::thread::sleep(Duration::from_secs(2));
    println!(
        "state {:?} navigations {}",
        view.state(),
        view.navigations()
    );
    // A second page of the same profile opens beside the first.
    let begun = std::time::Instant::now();
    let second = backend.open(
        &ViewOptions {
            profile: Some(PathBuf::from(profile)),
            width: 800,
            height: 600,
        },
        Arc::new(|_| {}),
    );
    println!(
        "second view {:?} in {:?}",
        second.as_ref().map(|_| ()),
        begun.elapsed()
    );
    if let Some(frame) = view.take_frame() {
        save(&frame, &format!("{prefix}-live.png"));
    }
    save(&view.capture().unwrap(), &format!("{prefix}-visible.png"));
    // A script to run in the isolated world: the fourth argument names a file that holds it.
    let call = arguments.get(4).map_or_else(
        || "document.title + ' ' + document.body.innerText.length".to_owned(),
        |path| std::fs::read_to_string(path).unwrap(),
    );
    let text = view.run_script(&call).unwrap();
    println!(
        "snapshot {} bytes: {}",
        text.len(),
        &text[..text.len().min(700)]
    );
    println!("cookies present: {:?}", view.cookies_present(url));
    // What a script keeps in its world is the page's to find if the page can see the world: a
    // page that reports `typeof window.<name>` in its title shows whether it does.
    println!(
        "set in the world: {:?}",
        view.run_script("window.__soyehtSeen = 1; String(typeof window.__soyehtSeen)")
    );
    std::thread::sleep(Duration::from_secs(2));
    println!("title after: {}", view.state().title);
    if embedded() {
        hidden_and_scrolled(&view, &placement, prefix);
    }
    // Time to look at the page on screen: the fifth argument is how many seconds.
    let hold = arguments.get(5).and_then(|seconds| seconds.parse().ok());
    std::thread::sleep(Duration::from_secs(hold.unwrap_or(0)));
}

/// What a reader needs when the pane is not on screen, and a wheel that moves the page.
fn hidden_and_scrolled(view: &WebView, placement: &dyn Fn(bool) -> Placement, prefix: &str) {
    let position = || view.run_script("String(window.scrollY)").unwrap();
    println!("scroll before: {}", position());
    view.scroll(
        ScrollInput {
            x: 400.0,
            y: 300.0,
            dx: 0.0,
            dy: 5.0,
        },
        Modifiers::default(),
    );
    std::thread::sleep(Duration::from_secs(1));
    println!("scroll after a wheel of 5 lines: {}", position());
    view.place(placement(false));
    std::thread::sleep(Duration::from_secs(1));
    println!("hidden, script: {:?}", view.run_script("document.title"));
    match view.capture() {
        Ok(frame) => save(&frame, &format!("{prefix}-hidden.png")),
        Err(error) => println!("hidden, capture: {error}"),
    }
    view.place(placement(true));
    std::thread::sleep(Duration::from_secs(1));
    println!("shown again, state {:?}", view.state());
}
