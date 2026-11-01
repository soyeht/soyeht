//! When the person or the system asks the process to stop, or the program's own file is gone
//! (`crate::program`).

use crate::{log, program};
use std::{future::Future, io};

/// Resolves when a stop is requested: interrupt or terminate on Unix; interrupt, break, console
/// close or system shutdown on Windows; and when the file the process runs from is gone (an app
/// moved to the Trash). Create it before the work it interrupts starts, so a
/// request that arrives in between is not missed. Call it inside a tokio runtime.
#[cfg(unix)]
pub fn requested() -> io::Result<impl Future<Output = ()>> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let gone = program_gone()?;
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
pub fn requested() -> io::Result<impl Future<Output = ()>> {
    use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_shutdown};
    let (mut close, mut shutdown, mut interrupt) = (ctrl_close()?, ctrl_shutdown()?, ctrl_break()?);
    let gone = program_gone()?;
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

/// Resolves when the file this process runs from is gone. If the watch cannot end (its thread
/// is lost) this never resolves: a lost watch must not stop the program.
fn program_gone() -> io::Result<impl Future<Output = ()>> {
    let (gone, resolved) = tokio::sync::oneshot::channel();
    program::on_gone(move || {
        log::event("program.gone", &[]);
        // A receiver that is gone means the stop is already under way.
        gone.send(()).unwrap_or(());
    })?;
    Ok(async move {
        if resolved.await.is_err() {
            std::future::pending::<()>().await;
        }
    })
}
