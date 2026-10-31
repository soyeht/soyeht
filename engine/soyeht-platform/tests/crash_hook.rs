//! The panic hook writes a report. The hook is process-wide state, so this is one test in a
//! process of its own.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::crash::{self, MAX_REPORT_BYTES, Program, Settings, store};
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

static PREVIOUS_HOOK_CALLS: AtomicUsize = AtomicUsize::new(0);

fn panic_on(name: &str) {
    let result = thread::Builder::new()
        .name(name.to_owned())
        .spawn(|| {
            let missing: Option<u8> = std::hint::black_box(None);
            missing.expect("the message names a secret project");
        })
        .unwrap()
        .join();
    assert!(result.is_err(), "the panic still unwinds to the join");
}

#[test]
fn a_panic_leaves_a_report_and_keeps_the_hook_that_was_there() {
    let data = std::env::temp_dir().join(format!("soyeht-crash-hook-{}", std::process::id()));
    let _ = fs::remove_dir_all(&data);
    let logs = data.join("logs");
    fs::create_dir_all(&logs).unwrap();
    let home = std::env::home_dir().unwrap_or_default();
    fs::write(
        logs.join("engine.log"),
        format!(
            "{{\"event\":\"engine.ready\"}}\n\
             {{\"event\":\"engine.opened\",\"path\":\"{}/Projects/secret.txt\"}}\n\
             thread 'main' panicked at x.rs:1:1:\nthe message names a secret project\n",
            home.display()
        ),
    )
    .unwrap();

    std::panic::set_hook(Box::new(|info| {
        PREVIOUS_HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
        eprintln!("{info}");
    }));
    crash::install(Settings {
        program: Program::Engine,
        data: data.clone(),
        log: "engine.log",
    });

    panic_on("main");
    let pending = store::pending(&data).unwrap();
    assert_eq!(pending.len(), 1);
    let report = &pending[0].report;
    assert_eq!(report.program, Program::Engine);
    assert!(report.fatal);
    assert_eq!(report.kind, "explicit");
    assert!(
        report.location.contains("crash_hook.rs"),
        "{}",
        report.location
    );
    assert!(!report.frames.is_empty());
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
    // Structured log lines only, redacted; the default hook's message line is not kept.
    assert_eq!(report.log.len(), 2, "{:?}", report.log);
    assert!(report.log[0].contains("engine.ready"));
    let stored = fs::read_to_string(store::directory(&data).join(&pending[0].name)).unwrap();
    assert!(stored.len() <= MAX_REPORT_BYTES);
    assert!(!stored.contains("secret"), "{stored}");
    if !home.as_os_str().is_empty() {
        assert!(!stored.contains(&home.display().to_string()), "{stored}");
    }
    assert_eq!(PREVIOUS_HOOK_CALLS.load(Ordering::Relaxed), 1);

    // A panic that only ends a task is reported but is not a closing.
    panic_on("tokio-runtime-worker");
    assert_eq!(store::list(&data).unwrap().len(), 2);
    assert_eq!(store::pending(&data).unwrap().len(), 1);

    // A program in a panic loop cannot push the older reports out.
    for _ in 0..10 {
        panic_on("worker");
    }
    assert_eq!(store::list(&data).unwrap().len(), 3);
    assert_eq!(PREVIOUS_HOOK_CALLS.load(Ordering::Relaxed), 12);

    fs::remove_dir_all(data).ok();
}
