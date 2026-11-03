//! When the person or the system asks the process to stop, or, on macOS, the program's own file
//! is gone (`crate::program`).

use crate::{log, program};
use std::{future::Future, io};

/// Resolves when a stop is requested: interrupt or terminate on Unix; interrupt, break, console
/// close or system shutdown on Windows; and, on macOS, when the file the process runs from is gone
/// (an app moved to the Trash). Create it before the work it interrupts starts, so a
/// request that arrives in between is not missed. Call it inside a tokio runtime.
pub fn requested() -> io::Result<impl Future<Output = ()>> {
    stop_or(program_gone(program::on_gone))
}

/// [`requested`] with the "program is gone" future given, so a test can drive it.
#[cfg(unix)]
fn stop_or(gone: impl Future<Output = ()>) -> io::Result<impl Future<Output = ()>> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => (),
            _ = terminate.recv() => (),
            () = gone => (),
        }
    })
}

/// See the Unix flavour.
#[cfg(windows)]
fn stop_or(gone: impl Future<Output = ()>) -> io::Result<impl Future<Output = ()>> {
    use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_shutdown};
    let (mut close, mut shutdown, mut interrupt) = (ctrl_close()?, ctrl_shutdown()?, ctrl_break()?);
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => (),
            () = gone => (),
            _ = close.recv() => (),
            _ = shutdown.recv() => (),
            _ = interrupt.recv() => (),
        }
    })
}

/// Resolves when `start`'s watch says the program's file is gone. A watch that cannot start, or
/// that ends without saying so, never resolves: a lost watch must not stop the program. The
/// failure is logged by its kind.
fn program_gone(
    start: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>,
) -> impl Future<Output = ()> {
    let (gone, resolved) = tokio::sync::oneshot::channel();
    let started = start(Box::new(move || {
        log::event("program.gone", &[]);
        // A receiver that is gone means the stop is already under way.
        gone.send(()).unwrap_or(());
    }));
    if let Err(error) = started {
        log::failure("program.watch_failed", &error);
    }
    async move {
        if resolved.await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn watched_file(label: &str) -> std::path::PathBuf {
        let folder =
            std::env::temp_dir().join(format!("next-shutdown-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("soyeht-engine");
        std::fs::write(&file, b"program").unwrap();
        file
    }

    #[tokio::test]
    async fn a_stop_is_requested_when_the_watched_file_is_deleted_and_not_before() {
        let file = watched_file("requested");
        let watched = file.clone();
        let gone = program_gone(move |action| {
            program::watch(watched, Duration::from_millis(20), 3, action)
        });
        let mut stop = Box::pin(stop_or(gone).unwrap());
        assert!(
            tokio::time::timeout(Duration::from_millis(300), &mut stop)
                .await
                .is_err(),
            "a file that is there ends nothing"
        );
        std::fs::remove_file(&file).unwrap();
        tokio::time::timeout(Duration::from_secs(10), stop)
            .await
            .unwrap();
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_watch_that_cannot_start_stops_nothing() {
        let gone = program_gone(|_| Err(io::Error::from(io::ErrorKind::PermissionDenied)));
        let stop = stop_or(gone).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(200), stop)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_watch_that_does_nothing_stops_nothing() {
        // What `program::on_gone` does off macOS: it returns Ok and drops the action.
        let gone = program_gone(|action| {
            drop(action);
            Ok(())
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(200), gone)
                .await
                .is_err()
        );
    }
}
