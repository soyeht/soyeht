//! When the person or the system asks the process to stop.

use std::{future::Future, io};

/// Resolves when a stop is requested: interrupt or terminate on Unix; interrupt, break, console
/// close or system shutdown on Windows. Create it before the work it interrupts starts, so a
/// request that arrives in between is not missed. Call it inside a tokio runtime.
#[cfg(unix)]
pub fn requested() -> io::Result<impl Future<Output = ()>> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => (),
            _ = terminate.recv() => (),
        }
    })
}

/// See the Unix flavour.
#[cfg(windows)]
pub fn requested() -> io::Result<impl Future<Output = ()>> {
    use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_shutdown};
    let (mut close, mut shutdown, mut interrupt) = (ctrl_close()?, ctrl_shutdown()?, ctrl_break()?);
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => (),
            _ = close.recv() => (),
            _ = shutdown.recv() => (),
            _ = interrupt.recv() => (),
        }
    })
}
