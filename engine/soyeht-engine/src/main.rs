#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_engine::{
    errors::{Error, Result},
    log_rotation::{self, LOG_KEEP_ROTATED, LOG_MAX_BYTES},
    server::{self, Config},
};
use soyeht_protocol::{data_directory::dev_data_directory, profile::DEV_TCP_PORT};
use std::{net::SocketAddr, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    // Bound the log the launch agent points stderr at; kept until `main` ends.
    let _log_rotation = match log_rotation::capture_stderr(LOG_MAX_BYTES, LOG_KEEP_ROTATED) {
        Ok(rotation) => rotation,
        Err(error) => {
            soyeht_platform::log::failure("engine.log_rotation_unavailable", &error);
            None
        }
    };
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
    if !listen.is_empty() {
        return server::run(Config {
            directory,
            listen,
            tailnet_port: None,
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
    })
    .await
}
