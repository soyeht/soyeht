//! The command line of an engine program: where its data lives and where it listens.
//!
//! `soyeht-engine` runs it with [`NoHooks`](crate::hooks::NoHooks); the test program runs it
//! with the hooks of [`crate::test_hooks`].

use crate::{
    errors::{Error, Result},
    hooks::Hooks,
    log_rotation::{self, LOG_KEEP_ROTATED, LOG_MAX_BYTES},
    server::{self, Config},
};
use soyeht_protocol::{data_directory::dev_data_directory, profile::DEV_TCP_PORT};
use std::{net::SocketAddr, path::Path, path::PathBuf, sync::Arc};

/// Runs the engine until it stops. `hooks` receives the data directory the arguments chose.
pub async fn run(hooks: impl FnOnce(&Path) -> Arc<dyn Hooks>) -> Result<()> {
    // Bound the log the launch agent points stderr at; kept until `run` ends.
    let _log_rotation = match log_rotation::capture_stderr(LOG_MAX_BYTES, LOG_KEEP_ROTATED) {
        Ok(rotation) => rotation,
        Err(error) => {
            soyeht_platform::log::failure("engine.log_rotation_unavailable", &error);
            None
        }
    };
    // Captures record the engine build; hashing the executable overlaps the rest of startup.
    crate::engine_build::prefetch_engine_build();
    let mut directory = dev_data_directory(std::env::home_dir())?;
    let mut listen = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                directory = PathBuf::from(args.next().ok_or(Error::MissingDataDirectoryArgument)?)
            }
            "--listen" => listen.push(
                args.next()
                    .ok_or(Error::MissingListenAddressArgument)?
                    .parse::<SocketAddr>()
                    .map_err(Error::Address)?,
            ),
            _ => return Err(Error::InvalidArguments),
        }
    }
    let hooks = hooks(&directory);
    if !listen.is_empty() {
        return server::run(Config {
            directory,
            listen,
            tailnet_port: None,
            hooks,
        })
        .await;
    }
    // Local clients always have loopback. Tailnet listeners follow the interfaces
    // inside the engine (ADR 0078): a missing address is logged, reported to clients
    // and retried, and a late address starts listening without a restart.
    server::run(Config {
        directory,
        listen: vec![SocketAddr::from(([127, 0, 0, 1], DEV_TCP_PORT))],
        tailnet_port: Some(DEV_TCP_PORT),
        hooks,
    })
    .await
}
