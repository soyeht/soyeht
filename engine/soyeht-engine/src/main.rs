#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_engine::{
    errors::{Error, Result},
    server::{self, Config},
};
use std::{net::SocketAddr, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    let mut directory = PathBuf::from(std::env::var_os("HOME").ok_or(Error::HomeMissing)?)
        .join("Library/Application Support/Soyeht.next/dev");
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
    listen.push(SocketAddr::from(([127, 0, 0, 1], 8291)));
    for ip in &addresses {
        listen.push(SocketAddr::new(*ip, 8291));
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
