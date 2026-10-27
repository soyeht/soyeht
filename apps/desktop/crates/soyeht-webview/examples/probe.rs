//! Opens one page in the system web view and reports what a reader gets from it. For trying the
//! web view on a machine: `probe PROFILE_DIR URL OUT_PREFIX`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use soyeht_webview::{Backend, Frame, Navigation, ViewOptions};
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
}
