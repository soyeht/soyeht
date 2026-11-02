//! A panic that is caught on purpose on the main thread does not end the program, so it is no
//! "closed unexpectedly" and cannot crowd out the report of the panic that does. The hook is
//! process-wide state, so this is one test in a process of its own.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::crash::{self, Program, Settings, store};
use std::{fs, thread};

/// Runs `body` on a thread named `main`, as the program's own main thread is.
fn on_main(body: impl FnOnce() + Send + 'static) -> thread::Result<()> {
    thread::Builder::new()
        .name("main".to_owned())
        .spawn(body)
        .unwrap()
        .join()
}

#[test]
fn a_caught_panic_on_the_main_thread_is_not_fatal_and_is_capped() {
    let data = std::env::temp_dir().join(format!("soyeht-crash-caught-{}", std::process::id()));
    let _ = fs::remove_dir_all(&data);
    std::panic::set_hook(Box::new(|_| {}));
    crash::install(Settings {
        program: Program::Desktop,
        data: data.clone(),
        log: "desktop.log",
    });

    // Five different panics (the same bytes would be one report), all caught.
    let messages: [&'static str; 5] = [
        "called `Option::unwrap()` on a `None` value",
        "index out of bounds: the len is 1 but the index is 2",
        "attempt to add with overflow",
        "already borrowed: BorrowMutError",
        "slice index starts at 3 but ends at 2",
    ];
    on_main(move || {
        for message in messages {
            let caught = crash::caught(|| std::panic::panic_any(message));
            assert!(caught.is_err());
        }
    })
    .unwrap();
    let written = store::list(&data).unwrap();
    assert_eq!(
        written.len(),
        3,
        "the cap of panics that do not end the program"
    );
    assert!(written.iter().all(|stored| !stored.report.fatal));
    assert!(
        store::pending(&data).unwrap().is_empty(),
        "nothing closed, so no notice"
    );

    // Nested, and after a caught call the thread is not inside one any more.
    let ended = on_main(|| {
        let outer = crash::caught(|| crash::caught(|| std::panic::panic_any("slice index nested")));
        assert!(outer.is_ok());
        std::panic::panic_any("the real end");
    });
    assert!(ended.is_err());
    let pending = store::pending(&data).unwrap();
    assert_eq!(
        pending.len(),
        1,
        "the panic that ends the program is still written"
    );
    assert_eq!(pending[0].report.kind, "explicit");
    fs::remove_dir_all(data).ok();
}
