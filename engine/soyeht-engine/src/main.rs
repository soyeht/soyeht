#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_engine::{
    errors::{Error, Result},
    log_rotation::{self, LOG_KEEP_ROTATED, LOG_MAX_BYTES},
    server::{self, Config},
};
use soyeht_protocol::profile::{DEV_DATA_DIRECTORY_FROM_HOME, DEV_TCP_PORT};
use std::{net::SocketAddr, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    // Bound the log the launch agent points stderr at; kept until `main` ends.
    let _log_rotation = match log_rotation::capture_stderr(LOG_MAX_BYTES, LOG_KEEP_ROTATED) {
        Ok(rotation) => rotation,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"event":"engine.log_rotation_unavailable","ioKind":format!("{:?}", error.kind())})
            );
            None
        }
    };
    let mut directory = PathBuf::from(std::env::var_os("HOME").ok_or(Error::HomeMissing)?)
        .join(DEV_DATA_DIRECTORY_FROM_HOME);
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
        return server::run(Config { directory, listen }).await;
    }
    let addresses = soyeht_engine::network::tailnet_addresses().unwrap_or_default();
    listen.push(SocketAddr::from(([127, 0, 0, 1], DEV_TCP_PORT)));
    for ip in &addresses {
        listen.push(SocketAddr::new(*ip, DEV_TCP_PORT));
    }
    tokio::select! {
        result = server::run(Config { directory, listen }) => result,
        _ = soyeht_engine::network::wait_for_address_change(addresses) => {
            // The Dev service manager restarts only the engine. The supervisor
            // and all live shells remain available for adoption.
            eprintln!("{}", serde_json::json!({"event":"engine.network_changed"}));
            Ok(())
        }
    }
}
